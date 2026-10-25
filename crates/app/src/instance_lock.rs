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
        if let Some(path) = &self.path {
            release_at(path);
        }
    }
}

/// Remove this process's lock, if it holds one, before exiting without closing the window
/// (`process::exit` skips the window-close release).
pub fn release_own() {
    if let Some(path) = config_file("tabs.lock") {
        release_at(&path);
    }
}

fn release_at(path: &Path) {
    let ours = fs::read_to_string(path)
        .ok()
        .and_then(|text| lock_pid(&text))
        == Some(process::id());
    if ours {
        let _ = fs::remove_file(path);
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
/// program name (so a reused pid doesn't count); elsewhere any running process with that pid,
/// so a lock left by a crash is taken over once its pid is free.
fn is_alive(pid: u32) -> bool {
    if !cfg!(target_os = "linux") {
        return crate::processes::is_running(pid);
    }
    let comm = |pid: &str| fs::read_to_string(format!("/proc/{pid}/comm")).ok();
    match (comm(&pid.to_string()), comm("self")) {
        (Some(theirs), Some(ours)) => theirs == ours,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "agentws-lock-{name}-{}-{}",
            process::id(),
            agent_status::now_micros()
        ));
        fs::create_dir_all(&folder).unwrap();
        folder.join("tabs.lock")
    }

    fn exited_pid() -> u32 {
        #[cfg(unix)]
        let mut child = process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let mut child = process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_crashed_owners_lock_is_taken_over() {
        let path = temp_lock("crashed");
        fs::write(&path, lock_text(exited_pid())).unwrap();
        assert!(try_create(&path).unwrap());
        assert_eq!(
            lock_pid(&fs::read_to_string(&path).unwrap()),
            Some(process::id())
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn release_removes_only_our_own_lock() {
        let path = temp_lock("release");
        fs::write(&path, lock_text(process::id() + 1)).unwrap();
        release_at(&path);
        assert!(path.is_file(), "another instance's lock stays");
        fs::write(&path, lock_text(process::id())).unwrap();
        release_at(&path);
        assert!(!path.exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
