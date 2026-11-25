use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub const PROGRESS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    version: u32,
    pub checkpoint: Option<String>,
    pub volume: f32,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            version: PROGRESS_SCHEMA_VERSION,
            checkpoint: None,
            volume: 0.7,
        }
    }
}

impl Progress {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.to_string()),
        };
        let progress: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        progress.validate()?;
        Ok(progress)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let path_dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(path_dir).map_err(|error| error.to_string())?;
        let file = tempfile::NamedTempFile::new_in(path_dir).map_err(|error| error.to_string())?;
        serde_json::to_writer(file.as_file(), self).map_err(|error| error.to_string())?;
        file.as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        file.persist(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != PROGRESS_SCHEMA_VERSION
            || !(0.0..=1.0).contains(&self.volume)
            || self
                .checkpoint
                .as_deref()
                .is_some_and(|id| !crate::game::valid_id(id))
        {
            return Err("Invalid game progress.".into());
        }
        Ok(())
    }
}

pub fn path(identifier: &str) -> Option<PathBuf> {
    if let Some(directory) = env::var_os("FORGE3D_SAVE_DIRECTORY").filter(|path| !path.is_empty()) {
        return Some(
            PathBuf::from(directory)
                .join(identifier)
                .join("progress.json"),
        );
    }
    if cfg!(target_os = "android") {
        return Some(
            PathBuf::from(env::var_os("FORGE3D_ANDROID_FILES_DIR")?).join("progress.json"),
        );
    }
    let home = || env::var_os("HOME").map(PathBuf::from);
    let directory = if cfg!(windows) {
        PathBuf::from(env::var_os("LOCALAPPDATA")?)
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support")
    } else {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home().map(|path| path.join(".local/share")))?
    };
    Some(directory.join(identifier).join("progress.json"))
}
