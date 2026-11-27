//! The editor builds an isolated scene snapshot and runs the game's own Cargo executable.
use crate::{component::Schema, export, game::Game};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub struct Compiled {
    _snapshot: tempfile::TempDir,
    pub executable: PathBuf,
    pub schemas: Vec<Schema>,
}

pub fn compile(game: &Game, offline: bool) -> Result<Compiled, String> {
    let source = game
        .rust_source
        .as_ref()
        .ok_or("Initialize the Rust project first.")?;
    let snapshot = tempfile::Builder::new()
        .prefix("forge-play-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let directory = snapshot.path().join("game");
    export::collection(game, &directory)?;
    let cargo = std::env::var_os("CARGO")
        .map(PathBuf::from)
        .or_else(|| {
            let candidate = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(PathBuf::from)
                .map(|home| {
                    home.join(".cargo/bin")
                        .join(if cfg!(windows) { "cargo.exe" } else { "cargo" })
                });
            candidate.filter(|p| p.is_file())
        })
        .or_else(|| {
            ["/opt/homebrew/bin/cargo", "/usr/local/bin/cargo"]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("cargo"));
    let mut command = Command::new(&cargo);
    if let Some(bin) = cargo.parent().filter(|p| p.is_absolute()) {
        let paths = std::iter::once(bin.to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .collect::<Vec<_>>(),
        );
        if let Ok(path) = std::env::join_paths(paths) {
            command.env("PATH", path);
        }
    }
    command
        .current_dir(&directory)
        .args(["build", "--bin", "forge-game", "--message-format=json"])
        .arg("--target-dir")
        .arg(
            source
                .parent()
                .ok_or("Invalid game-code path.")?
                .join("target"),
        );
    if offline {
        command.arg("--offline");
    }
    let output = command
        .output()
        .map_err(|error| format!("Could not start Cargo: {error}"))?;
    let mut executable = None;
    let mut diagnostics = String::from_utf8_lossy(&output.stderr).into_owned();
    for line in output.stdout.split(|&b| b == b'\n') {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        match value["reason"].as_str() {
            Some("compiler-message") => {
                if let Some(message) = value["message"]["rendered"].as_str() {
                    diagnostics.push_str(message);
                }
            }
            Some("compiler-artifact") if value["target"]["name"] == "forge-game" => {
                executable = value["executable"].as_str().map(PathBuf::from);
            }
            _ => {}
        }
    }
    if !output.status.success() {
        return Err(format!(
            "Rust build failed ({}).\n{diagnostics}",
            source.display()
        ));
    }
    // Cargo spans use snapshot-relative filenames; pair them with the editable root.
    let built = executable.ok_or("Cargo produced no game executable.")?;
    // Launch from a private copy so rebuilding can replace Cargo's output while
    // an earlier Windows player is still running.
    let executable = snapshot
        .path()
        .join(built.file_name().ok_or("Invalid Cargo executable path.")?);
    std::fs::copy(built, &executable).map_err(|e| e.to_string())?;
    let report = Command::new(&executable)
        .arg("--component-schemas")
        .output()
        .map_err(|e| e.to_string())?;
    if !report.status.success() {
        return Err(String::from_utf8_lossy(&report.stderr).into_owned());
    }
    let schemas = serde_json::from_slice(&report.stdout)
        .map_err(|e| format!("Invalid component schema report: {e}"))?;
    Ok(Compiled {
        _snapshot: snapshot,
        executable,
        schemas,
    })
}

impl Compiled {
    pub fn directory(&self) -> &Path {
        self._snapshot.path()
    }

    pub fn check(&self) -> Result<(), String> {
        let output = Command::new(&self.executable)
            .arg("--check-scene")
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }
}
