//! Which running instance owns the saved tabs. The first instance to start writes its pid to a
//! lock file in the config folder; while that process lives, later instances neither reopen
//! nor save the tabs, so they don't duplicate its sessions or overwrite its save.

/// The lock file's content for the process `pid`.
pub fn lock_text(pid: u32) -> String {
    format!("{pid}\n")
}

/// The pid a lock file names; `None` when it names none (empty, damaged).
pub fn lock_pid(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

/// Whether the lock file `text` is held by another live instance. A lock naming no pid, this
/// process (`own_pid`) or a process that `alive` says is gone is stale and can be taken over.
pub fn held_by_other(text: &str, own_pid: u32, alive: impl FnOnce(u32) -> bool) -> bool {
    lock_pid(text).is_some_and(|pid| pid != own_pid && alive(pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_text_round_trip() {
        assert_eq!(lock_pid(&lock_text(4242)), Some(4242));
        assert_eq!(lock_pid(" 17 \n"), Some(17));
        assert_eq!(lock_pid(""), None);
        assert_eq!(lock_pid("abc"), None);
        assert_eq!(lock_pid("-3"), None);
    }

    #[test]
    fn a_live_other_instance_holds_the_lock() {
        assert!(held_by_other("100\n", 200, |pid| pid == 100));
    }

    #[test]
    fn stale_locks_can_be_taken_over() {
        // The process is gone.
        assert!(!held_by_other("100\n", 200, |_| false));
        // Our own pid (left by an earlier process that had it).
        assert!(!held_by_other("200\n", 200, |_| true));
        // Nothing usable in the file.
        assert!(!held_by_other("", 200, |_| true));
        assert!(!held_by_other("garbage", 200, |_| true));
    }
}
