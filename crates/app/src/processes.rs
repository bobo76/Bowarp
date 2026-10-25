//! Questions about other processes: whether one still runs, and who this one runs as.

#[cfg(unix)]
use std::io;

/// Whether a process with `pid` may still run. Only a pid known to be free counts as gone:
/// one the check can't answer for (permission denied) counts as running.
#[cfg(unix)]
pub fn is_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // SAFETY: `kill` has no memory-safety preconditions; signal zero only checks that the pid
    // exists, it sends nothing.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Whether a process with `pid` may still run. Only a pid known to be free counts as gone:
/// one the check can't answer for (access denied) counts as running.
#[cfg(windows)]
pub fn is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: `OpenProcess` takes no pointers; a null handle is checked before use.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // SAFETY: reads this thread's last error, right after the failed call.
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut code = 0;
    // SAFETY: `handle` is a valid process handle opened above and `code` outlives the call.
    let queried = unsafe { GetExitCodeProcess(handle, &mut code) } != 0;
    // SAFETY: `handle` is valid and closed exactly once, here.
    unsafe { CloseHandle(handle) };
    !queried || code == STILL_ACTIVE as u32
}

/// The user this process runs as.
#[cfg(unix)]
pub fn effective_uid() -> libc::uid_t {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_processes_run_and_exited_ones_do_not() {
        assert!(is_running(std::process::id()));
        #[cfg(unix)]
        let mut command = std::process::Command::new("sh");
        #[cfg(windows)]
        let mut command = std::process::Command::new("cmd");
        #[cfg(unix)]
        command.args(["-c", "exit 0"]);
        #[cfg(windows)]
        command.args(["/C", "exit 0"]);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        assert!(!is_running(pid));
    }
}
