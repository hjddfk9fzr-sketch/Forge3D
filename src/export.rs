use crate::{game::Game, runtime::Scene, scripting::Scripts};
use std::{fs, io, path::Path};

pub fn game(scene: &Scene, directory: &Path) -> Result<(), String> {
    collection(&Game::single(scene.clone()), directory)
}

pub fn collection(game: &Game, directory: &Path) -> Result<(), String> {
    staged_game(game, directory, None, || false)
}

/// Checks the same scene and script requirements as an actual export,
/// while leaving the input and filesystem untouched.
pub fn validate(game: &Game) -> Result<(), String> {
    validated_game(game).map(|_| ())
}

fn validated_game(game: &Game) -> Result<Game, String> {
    game.start_scene()?;
    game.validate_components()?;
    let mut game = game.clone();
    for (id, scene) in game.scenes.iter_mut().chain(&mut game.prefabs) {
        if !crate::game::valid_id(id) {
            return Err(format!("Invalid scene ID '{id}'."));
        }
        scene
            .assign_ids()
            .and_then(|()| scene.validate())
            .map_err(|error| format!("{id}: {error}"))?;
        Scripts::compile(scene).map_err(|error| format!("{id}: {error}"))?;
    }
    Ok(game)
}

pub(crate) fn staged_game(
    game: &Game,
    directory: &Path,
    plan: Option<&[u8]>,
    cancelled: impl FnOnce() -> bool,
) -> Result<(), String> {
    let mut game = validated_game(game)?;
    let scene = game
        .scenes
        .remove(&game.start)
        .ok_or("Missing starting scene.")?;
    let data = serde_json::to_vec(&scene).map_err(|error| error.to_string())?;
    let additional = serde_json::to_vec(&game).map_err(|error| error.to_string())?;
    let app = &scene.application;
    let android = serde_json::to_vec_pretty(&serde_json::json!({
        "label":app.display_title(&scene.title),
        "identifier":app.identifier,
        "version":app.version,
        "version_code":app.version_code,
        "orientation":app.orientation,
        "fullscreen":app.android_fullscreen
    }))
    .map_err(|error| error.to_string())?;
    let cargo = include_str!("../templates/game.toml").replace("FORGE_GAME_VERSION", &app.version);

    publish(directory, cancelled, |directory| {
        fs::create_dir(directory.join("src"))?;
        fs::create_dir(directory.join("assets"))?;
        fs::create_dir_all(directory.join("android/src/main"))?;
        fs::write(directory.join("Cargo.toml"), cargo)?;
        fs::write(
            directory.join("README.md"),
            include_str!("../templates/game-readme.md"),
        )?;
        fs::write(
            directory.join(".gitignore"),
            "/target/\n/android/build/\n/android/.gradle/\n/android/local.properties\n",
        )?;
        fs::write(
            directory.join("src/main.rs"),
            include_str!("../templates/game.rs"),
        )?;
        fs::write(
            directory.join("src/lib.rs"),
            include_str!("../templates/game-lib.rs"),
        )?;
        fs::write(directory.join("src/runtime.rs"), include_str!("runtime.rs"))?;
        fs::write(
            directory.join("src/gameplay.rs"),
            include_str!("gameplay.rs"),
        )?;
        fs::write(
            directory.join("src/workshop.rs"),
            include_str!("workshop.rs"),
        )?;
        fs::write(directory.join("src/relay.rs"), include_str!("relay.rs"))?;
        fs::write(directory.join("src/game.rs"), include_str!("game.rs"))?;
        fs::write(
            directory.join("src/settings.rs"),
            include_str!("settings.rs"),
        )?;
        fs::write(
            directory.join("src/animation.rs"),
            include_str!("animation.rs"),
        )?;
        fs::write(
            directory.join("src/lighting.rs"),
            include_str!("lighting.rs"),
        )?;
        fs::write(
            directory.join("src/performance.rs"),
            include_str!("performance.rs"),
        )?;
        fs::write(directory.join("src/physics.rs"), include_str!("physics.rs"))?;
        fs::write(
            directory.join("src/presentation.rs"),
            include_str!("presentation.rs"),
        )?;
        fs::write(
            directory.join("src/progress.rs"),
            include_str!("progress.rs"),
        )?;
        fs::write(
            directory.join("src/controls.rs"),
            include_str!("controls.rs"),
        )?;
        fs::write(
            directory.join("src/scripting.rs"),
            include_str!("scripting.rs"),
        )?;
        fs::write(
            directory.join("android/settings.gradle"),
            include_str!("../templates/android/settings.gradle"),
        )?;
        fs::write(
            directory.join("android/build.gradle"),
            include_str!("../templates/android/build.gradle"),
        )?;
        fs::write(
            directory.join("android/src/main/AndroidManifest.xml"),
            include_str!("../templates/android/AndroidManifest.xml"),
        )?;
        fs::write(directory.join("android/forge-app.json"), android)?;
        fs::write(directory.join("assets/scene.json"), data)?;
        fs::write(directory.join("assets/scenes.json"), additional)?;
        if let Some(plan) = plan {
            fs::write(directory.join("forge-build-plan.json"), plan)?;
        }
        Ok(())
    })
    .map_err(|error| format!("Could not export to {}: {error}", directory.display()))
}

fn publish(
    destination: &Path,
    cancelled: impl FnOnce() -> bool,
    write: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".forge-export-")
        .tempdir_in(parent)?;
    write(staging.path())?;
    if cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Export cancelled.",
        ));
    }
    rename_new(staging.path(), destination)?;
    let _ = staging.keep();
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, source, CWD, destination, RenameFlags::NOREPLACE)?;
    Ok(())
}

#[cfg(windows)]
fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let source: Vec<_> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<_> = destination.as_os_str().encode_wide().chain([0]).collect();
    if destination[..destination.len() - 1].contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid destination path.",
        ));
    }
    // Both UTF-16 buffers are NUL terminated and remain valid during the call.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_or_cancelled_export_keeps_the_destination_absent() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("game");
        assert!(publish(
            &destination,
            || false,
            |stage| {
                fs::write(stage.join("partial"), "data")?;
                Err(io::Error::other("interrupted write"))
            }
        )
        .is_err());
        assert!(publish(
            &destination,
            || true,
            |stage| fs::write(stage.join("partial"), "data")
        )
        .is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn concurrent_destination_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("game");
        assert!(publish(
            &destination,
            || false,
            |stage| {
                fs::write(stage.join("partial"), "data")?;
                fs::create_dir(&destination)
            }
        )
        .is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}
