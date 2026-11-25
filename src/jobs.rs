use crate::{export, game::Game, runtime::Scene, settings::Application};
use command_group::CommandGroup;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Check,
    Test,
    Debug,
    Release,
    AndroidDebug,
    AndroidRelease,
    AndroidBundle,
}

impl Profile {
    fn gradle_task(self) -> Option<&'static str> {
        match self {
            Self::AndroidDebug => Some("assembleDebug"),
            Self::AndroidRelease => Some("assembleRelease"),
            Self::AndroidBundle => Some("bundleRelease"),
            _ => None,
        }
    }

    fn signed(self) -> bool {
        matches!(self, Self::AndroidRelease | Self::AndroidBundle)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    WindowsX64,
    MacosArm64,
    LinuxX64,
}

impl Target {
    pub fn triple(self) -> &'static str {
        match self {
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
        }
    }

    fn os(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows",
            Self::MacosArm64 => "macos",
            Self::LinuxX64 => "linux",
        }
    }
}

const SIGNING: [&str; 4] = [
    "FORGE3D_ANDROID_KEYSTORE",
    "FORGE3D_ANDROID_KEY_ALIAS",
    "FORGE3D_ANDROID_STORE_PASSWORD",
    "FORGE3D_ANDROID_KEY_PASSWORD",
];

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Plan {
    profile: Profile,
    target: Option<Target>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Toolchain,
    Export,
    Build,
    Package,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Toolchain => "toolchain",
            Self::Export => "export",
            Self::Build => "build",
            Self::Package => "package",
        }
    }
}

pub struct Job {
    directory: PathBuf,
    game: Arc<Game>,
    plan: Plan,
    cancelled: Arc<AtomicBool>,
    stages: Receiver<Stage>,
    stage: Stage,
    receiver: Receiver<Report>,
    report: Option<Report>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone, Serialize)]
pub struct Report {
    pub state: &'static str,
    pub stage: Stage,
    pub code: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub artifacts: Vec<PathBuf>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

impl Job {
    pub fn start(scene: &Scene, directory: &Path, profile: Profile) -> Result<Self, String> {
        Self::start_for(scene, directory, profile, None)
    }

    pub fn start_for(
        scene: &Scene,
        directory: &Path,
        profile: Profile,
        target: Option<Target>,
    ) -> Result<Self, String> {
        Self::start_game(Game::single(scene.clone()), directory, profile, target)
    }

    pub fn start_game(
        game: Game,
        directory: &Path,
        profile: Profile,
        target: Option<Target>,
    ) -> Result<Self, String> {
        Self::spawn(Arc::new(game), directory, Plan { profile, target })
    }

    fn spawn(game: Arc<Game>, directory: &Path, plan: Plan) -> Result<Self, String> {
        if plan.profile.gradle_task().is_some() && plan.target.is_some() {
            return Err("Desktop targets apply only to Cargo profiles.".into());
        }
        let name = directory
            .file_name()
            .ok_or("Expected a new build folder.")?;
        let parent = directory
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let directory = parent
            .canonicalize()
            .map_err(|error| format!("Could not open {}: {error}", parent.display()))?
            .join(name);
        let destination = directory.clone();
        let worker_game = game.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (sender, receiver) = mpsc::channel();
        let (stages_tx, stages) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("forge3d-build".into())
            .spawn(move || {
                let result = run(
                    &destination,
                    plan,
                    &worker_game,
                    &worker_cancelled,
                    &stages_tx,
                );
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            directory,
            game,
            plan,
            cancelled,
            stages,
            stage: Stage::Toolchain,
            receiver,
            report: None,
            worker: Some(worker),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn cancel(&mut self) -> bool {
        if self.status().is_some() {
            return false;
        }
        self.cancelled.store(true, Ordering::Relaxed);
        true
    }

    pub fn retry(&mut self, directory: &Path) -> Result<Self, String> {
        if self.status().is_none() {
            return Err("Wait for the build to finish before retrying.".into());
        }
        Self::spawn(self.game.clone(), directory, self.plan)
    }

    pub fn retry_export(previous: &Path, directory: &Path) -> Result<Self, String> {
        let plan: Plan = serde_json::from_slice(
            &fs::read(previous.join("forge-build-plan.json")).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let start =
            fs::read(previous.join("assets/scene.json")).map_err(|error| error.to_string())?;
        let additional = match fs::read(previous.join("assets/scenes.json")) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let game = Game::decode(&start, additional.as_deref())?;
        Self::spawn(Arc::new(game), directory, plan)
    }

    pub fn status(&mut self) -> Option<&Report> {
        if self.report.is_none() {
            for stage in self.stages.try_iter() {
                self.stage = stage;
            }
            match self.receiver.try_recv() {
                Ok(report) => {
                    self.stage = report.stage;
                    self.report = Some(report);
                }
                Err(TryRecvError::Disconnected) => {
                    self.report = Some(failed(
                        self.stage,
                        "worker_stopped",
                        "Build worker stopped.",
                    ));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.report.as_ref()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn check_signing() -> Result<(), String> {
    let missing: Vec<_> = SIGNING
        .into_iter()
        .filter(|name| std::env::var_os(name).is_none_or(|value| value.is_empty()))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Set Android release signing variables: {}.",
            missing.join(", ")
        ));
    }
    let key = std::env::var_os(SIGNING[0]).map(PathBuf::from);
    if !key.is_some_and(|path| path.is_absolute() && path.is_file()) {
        return Err("Use an absolute path to the Android release keystore.".into());
    }
    Ok(())
}

fn run(
    directory: &Path,
    plan: Plan,
    game: &Game,
    cancelled: &AtomicBool,
    stages: &Sender<Stage>,
) -> Report {
    let stopping = || cancelled.load(Ordering::Relaxed);
    if stopping() {
        return cancelled_report(Stage::Toolchain);
    }
    if plan.profile.signed() {
        if let Err(error) = check_signing() {
            return failed(Stage::Toolchain, "android_signing", error);
        }
    }

    let _ = stages.send(Stage::Export);
    let data = match serde_json::to_vec(&plan) {
        Ok(data) => data,
        Err(error) => return failed(Stage::Export, "export_failed", error.to_string()),
    };
    if let Err(error) = export::staged_game(game, directory, Some(&data), stopping) {
        return if stopping() {
            cancelled_report(Stage::Export)
        } else {
            failed(Stage::Export, "export_failed", error)
        };
    }
    if stopping() {
        return cancelled_report(Stage::Export);
    }

    let mut command = build_command(plan, directory);
    let stdout = directory.join("forge-build.stdout.log");
    let stderr = directory.join("forge-build.stderr.log");
    let mut report = match run_command(&mut command, &stdout, &stderr, cancelled, stages) {
        Ok(status) if status.success() => {
            let _ = stages.send(Stage::Package);
            if stopping() {
                cancelled_report(Stage::Package)
            } else {
                let packed = game.start_scene().and_then(|scene| {
                    artifacts(
                        directory,
                        plan.profile,
                        plan.target,
                        &scene.application,
                        &scene.application.display_title(&scene.title),
                    )
                });
                match packed {
                    _ if stopping() => cancelled_report(Stage::Package),
                    Ok(artifacts) => Report {
                        state: "succeeded",
                        stage: Stage::Package,
                        code: None,
                        exit_code: status.code(),
                        artifacts,
                        error: None,
                        stdout: String::new(),
                        stderr: String::new(),
                    },
                    Err(error) => failed(Stage::Package, "packaging_failed", error),
                }
            }
        }
        Ok(status) => {
            let mut report = failed(
                Stage::Build,
                "toolchain_failed",
                "Build tool reported an error.",
            );
            report.exit_code = status.code();
            report
        }
        Err(report) => *report,
    };
    report.stdout = tail(&stdout);
    report.stderr = tail(&stderr);
    if plan.profile.signed()
        && report.code == Some("toolchain_failed")
        && signing_failure(&report.stderr)
    {
        report.code = Some("android_signing");
        report.error = Some(
            "Gradle could not sign the Android release. Check the keystore, alias and passwords."
                .into(),
        );
    }
    report
}

fn signing_failure(log: &str) -> bool {
    let log = log.to_ascii_lowercase();
    [
        "validatesigningrelease",
        "signreleasebundle",
        "failed to read key",
        "keystore password was incorrect",
        "no key with alias",
    ]
    .iter()
    .any(|message| log.contains(message))
}

fn build_command(plan: Plan, directory: &Path) -> Command {
    let profile = plan.profile;
    let mut command = if let Some(task) = profile.gradle_task() {
        let mut command = Command::new(std::env::var_os("FORGE3D_GRADLE").unwrap_or_else(|| {
            if cfg!(windows) {
                "gradle.bat".into()
            } else {
                "gradle".into()
            }
        }));
        command.args(["--no-daemon", "-p", "android", task]);
        command
    } else {
        let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        match profile {
            Profile::Check => command.arg("check"),
            Profile::Test => command.arg("test"),
            Profile::Debug => command.arg("build"),
            Profile::Release => command.args(["build", "--release"]),
            _ => unreachable!(),
        };
        if let Some(target) = plan.target {
            command.args(["--target", target.triple()]);
        }
        command
    };
    if !profile.signed() {
        for name in SIGNING {
            command.env_remove(name);
        }
    }
    command
        .current_dir(directory)
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .stdin(Stdio::null());
    command
}

fn run_command(
    command: &mut Command,
    stdout: &Path,
    stderr: &Path,
    cancelled: &AtomicBool,
    stages: &Sender<Stage>,
) -> Result<ExitStatus, Box<Report>> {
    let logs = |path: &Path| -> Result<File, Box<Report>> {
        File::create(path).map_err(|error| {
            failed(
                Stage::Build,
                "log_failed",
                format!("Could not open {}: {error}", path.display()),
            )
            .into()
        })
    };
    command.stdout(logs(stdout)?).stderr(logs(stderr)?);
    let program = command.get_program().to_string_lossy().into_owned();
    #[cfg(windows)]
    let child = command.group().kill_on_drop(true).spawn();
    #[cfg(unix)]
    let child = command.group_spawn();
    let mut child = child.map_err(|error| {
        let code = if error.kind() == io::ErrorKind::NotFound {
            "missing_toolchain"
        } else {
            "toolchain_start_failed"
        };
        failed(
            Stage::Toolchain,
            code,
            format!("Could not start {program}: {error}"),
        )
    })?;
    let _ = stages.send(Stage::Build);
    let start = Instant::now();
    let report = loop {
        let stopped = cancelled.load(Ordering::Relaxed);
        if stopped || start.elapsed() >= Duration::from_secs(900) {
            break if stopped {
                cancelled_report(Stage::Build)
            } else {
                failed(Stage::Build, "timed_out", "Build exceeded 15 minutes.")
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => break failed(Stage::Build, "toolchain_wait_failed", error.to_string()),
        }
    };
    if let Err(error) = child.kill() {
        if error.kind() != io::ErrorKind::NotFound && !matches!(child.try_wait(), Ok(Some(_))) {
            return Err(failed(Stage::Build, "termination_failed", error.to_string()).into());
        }
    }
    child
        .wait()
        .map_err(|error| failed(Stage::Build, "termination_failed", error.to_string()))?;
    Err(Box::new(report))
}

fn cancelled_report(stage: Stage) -> Report {
    Report {
        state: "cancelled",
        ..failed(stage, "cancelled", "Build cancelled.")
    }
}

fn failed(stage: Stage, code: &'static str, error: impl Into<String>) -> Report {
    Report {
        state: "failed",
        stage,
        code: Some(code),
        exit_code: None,
        artifacts: Vec::new(),
        stdout: String::new(),
        stderr: String::new(),
        error: Some(error.into()),
    }
}

fn artifacts(
    root: &Path,
    profile: Profile,
    target: Option<Target>,
    app: &Application,
    title: &str,
) -> Result<Vec<PathBuf>, String> {
    match profile {
        Profile::Check | Profile::Test => Ok(Vec::new()),
        Profile::AndroidDebug | Profile::AndroidRelease | Profile::AndroidBundle => {
            let (relative, extension) = match profile {
                Profile::AndroidDebug => ("apk/debug", "apk"),
                Profile::AndroidRelease => ("apk/release", "apk"),
                _ => ("bundle/release", "aab"),
            };
            let directory = root.join("android/build/outputs").join(relative);
            let mut paths: Vec<_> = fs::read_dir(directory)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file() && path.extension().is_some_and(|value| value == extension)
                })
                .collect();
            paths.sort();
            if paths.is_empty() {
                return Err(format!("Gradle did not produce a .{extension} file."));
            }
            Ok(paths)
        }
        Profile::Debug | Profile::Release => {
            let mode = if profile == Profile::Release {
                "release"
            } else {
                "debug"
            };
            let os = target.map_or(std::env::consts::OS, Target::os);
            let suffix = if os == "windows" { ".exe" } else { "" };
            let output = root
                .join("target")
                .join(target.map_or("", Target::triple))
                .join(mode)
                .join(format!("forge-game{suffix}"));
            if !output.is_file() {
                return Err("Cargo did not produce the game executable.".into());
            }
            if profile == Profile::Debug {
                return Ok(vec![output]);
            }
            let dist = root.join("dist");
            fs::create_dir(&dist).map_err(|error| error.to_string())?;
            let slug = app.identifier.rsplit('.').next().unwrap_or("forge-game");
            if os == "macos" {
                let path = dist.join(format!("{slug}.app.zip"));
                write_macos_bundle(&path, &output, app, title, slug)?;
                Ok(vec![path])
            } else {
                let path = dist.join(format!("{slug}{suffix}"));
                fs::copy(output, &path).map_err(|error| error.to_string())?;
                Ok(vec![path])
            }
        }
    }
}

/// Package a previously compiled editor. The OS and architecture are chosen
/// at build time by the caller, using the corresponding Cargo target.
pub fn package_editor(executable: &Path, destination: &Path, target: Target) -> Result<(), String> {
    if !executable.is_file() {
        return Err(format!(
            "Editor binary not found at {}.",
            executable.display()
        ));
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    if target.os() == "macos" {
        let app = Application {
            identifier: "org.forge3d.editor".into(),
            version: crate::compatibility::ENGINE_VERSION.into(),
            ..Application::default()
        };
        write_macos_bundle(temporary.path(), executable, &app, "Forge3D", "Forge3D")?;
    } else {
        fs::copy(executable, temporary.path()).map_err(|error| error.to_string())?;
    }
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn write_macos_bundle(
    path: &Path,
    executable: &Path,
    app: &Application,
    title: &str,
    slug: &str,
) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let run = || -> Result<(), Box<dyn std::error::Error>> {
        let mut archive = zip::ZipWriter::new(File::create(path)?);
        let base = format!("{slug}.app/Contents");
        archive.start_file(
            format!("{base}/Info.plist"),
            SimpleFileOptions::default().unix_permissions(0o644),
        )?;
        let title = title
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let version = semver::Version::parse(&app.version)?;
        write!(
            archive,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{}</string>
<key>CFBundleName</key><string>{title}</string>
<key>CFBundleDisplayName</key><string>{title}</string>
<key>CFBundleExecutable</key><string>{slug}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{}.{}.{}</string>
<key>CFBundleVersion</key><string>{}</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#,
            app.identifier, version.major, version.minor, version.patch, app.version_code
        )?;
        archive.start_file(
            format!("{base}/MacOS/{slug}"),
            SimpleFileOptions::default().unix_permissions(0o755),
        )?;
        io::copy(&mut File::open(executable)?, &mut archive)?;
        archive.finish()?;
        Ok(())
    };
    run().map_err(|error| error.to_string())
}

fn tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |value| value.len());
    if file
        .seek(SeekFrom::Start(len.saturating_sub(16_384)))
        .is_err()
    {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.take(16_384).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_the_tool_and_its_child() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let cancelled = AtomicBool::new(false);
        let (stages, _) = mpsc::channel();
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                let mut tool = Command::new("sh");
                tool.current_dir(root)
                    .args(["-c", "(touch started; sleep 1; touch orphan) & wait"]);
                run_command(
                    &mut tool,
                    &root.join("stdout"),
                    &root.join("stderr"),
                    &cancelled,
                    &stages,
                )
            });
            let start = Instant::now();
            while !root.join("started").exists() && start.elapsed() < Duration::from_secs(5) {
                thread::sleep(Duration::from_millis(10));
            }
            cancelled.store(true, Ordering::Relaxed);
            assert!(root.join("started").exists());
            let report = worker.join().unwrap().expect_err("expected cancellation");
            assert_eq!(report.state, "cancelled");
            assert_eq!(report.code, Some("cancelled"));
            thread::sleep(Duration::from_secs(1));
            assert!(!root.join("orphan").exists());
        });
    }

    #[test]
    fn macos_archive_uses_a_bundle_executable_and_escaped_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("forge-game");
        fs::write(&binary, b"fixture").unwrap();
        let archive = directory.path().join("demo.app.zip");
        let app = Application {
            identifier: "com.forge3d.motion".into(),
            version: "2.0.1-beta.1".into(),
            version_code: 42,
            ..Application::default()
        };
        write_macos_bundle(&archive, &binary, &app, "Light & <Motion>", "motion").unwrap();
        let mut contents = zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        let mut plist = String::new();
        contents
            .by_name("motion.app/Contents/Info.plist")
            .unwrap()
            .read_to_string(&mut plist)
            .unwrap();
        assert!(plist.contains("Light &amp; &lt;Motion&gt;"));
        assert!(plist.contains("<string>com.forge3d.motion</string>"));
        assert!(plist.contains("<string>2.0.1</string>"));
        let executable = contents
            .by_name("motion.app/Contents/MacOS/motion")
            .unwrap();
        assert_eq!(executable.unix_mode().unwrap() & 0o777, 0o755);
    }
}
