//! The lock that lets one running instance own the saved tabs (`workspace_model::held_by_other`):
//! `tabs.lock` in the config folder, holding the owner's pid.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

use workspace_model::{held_by_other, lock_pid, lock_text};

use crate::window_bounds::config_file;

/// Held by the instance that reopens and saves the tabs.
pub struct InstanceLock {
    /// `None` when the lock file could not be written; this instance owns the tabs anyway.
    path: Option<PathBuf>,
}

impl InstanceLock {
    /// The lock, unless another live instance holds it. A lock left by a process that is gone
    /// is taken over. Two instances starting at the same moment over a stale lock may both get
    /// it; that only brings back the old race between their saves.
    pub fn acquire() -> Option<Self> {
        let Some(path) = config_file("tabs.lock") else {
            return Some(Self { path: None });
        };
        match try_create(&path) {
            Ok(owned) => owned.then_some(Self { path: Some(path) }),
            Err(err) => {
                eprintln!("agent-workspace: failed to write {}: {err}", path.display());
                Some(Self { path: None })
            }
        }
    }

    /// Remove the lock file, if it is still ours, so the next start owns the tabs at once.
    pub fn release(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let ours = fs::read_to_string(path)
            .ok()
            .and_then(|text| lock_pid(&text))
            == Some(process::id());
        if ours {
            let _ = fs::remove_file(path);
        }
    }
}

/// Create the lock file at `path` with our pid. `Ok(false)` when another live instance holds it.
fn try_create(path: &Path) -> io::Result<bool> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // A second try after removing a stale lock.
    for _ in 0..2 {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(lock_text(process::id()).as_bytes())?;
                return Ok(true);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                // Unreadable counts as empty, i.e. stale.
                let text = fs::read_to_string(path).unwrap_or_default();
                if held_by_other(&text, process::id(), is_alive) {
                    return Ok(false);
                }
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}

/// Whether `pid` is a running instance of this app. On Linux, a process with that pid and our
/// program name (so a reused pid doesn't count); elsewhere every pid counts as alive, so a lock
/// left by a crash must be removed by hand.
fn is_alive(pid: u32) -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let comm = |pid: &str| fs::read_to_string(format!("/proc/{pid}/comm")).ok();
    match (comm(&pid.to_string()), comm("self")) {
        (Some(theirs), Some(ours)) => theirs == ours,
        (Some(_), None) => true,
        (None, _) => false,
    }
}
