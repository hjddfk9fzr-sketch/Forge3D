use forge3d::{
    author::{self, Compiled},
    game::Game,
};
use std::{
    fs::File,
    process::{Child, Command, Stdio},
    sync::mpsc,
};

pub struct Build {
    pub launch: bool,
    receiver: mpsc::Receiver<Result<Compiled, String>>,
}

impl Build {
    pub fn start(game: Game, launch: bool) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("forge3d-project-rust".into())
            .spawn(move || {
                let _ = sender.send(author::compile(&game, false));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { launch, receiver })
    }

    pub fn poll(&self) -> Option<Result<Compiled, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Rust build worker stopped.".into())),
        }
    }
}

pub struct Preview {
    compiled: Compiled,
    child: Child,
}

impl Preview {
    pub fn launch(compiled: Compiled) -> Result<Self, String> {
        let log =
            File::create(compiled.directory().join("player.log")).map_err(|e| e.to_string())?;
        let error = log.try_clone().map_err(|e| e.to_string())?;
        let child = Command::new(&compiled.executable)
            .current_dir(compiled.directory())
            .arg("--preview")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Self { compiled, child })
    }

    pub fn poll(&mut self) -> Option<Result<(), String>> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) if status.success() => Some(Ok(())),
            Ok(Some(status)) => Some(Err(format!(
                "Rust player exited ({status}).\n{}",
                std::fs::read_to_string(self.compiled.directory().join("player.log"))
                    .unwrap_or_default()
            ))),
            Err(e) => Some(Err(e.to_string())),
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
