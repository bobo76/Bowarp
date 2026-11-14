//! Who the app is to a server ([`ClientIdentity`]): a random id made once per install and
//! kept in a file, and the machine's name.

use std::collections::hash_map::RandomState;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::protocol::ClientIdentity;

/// Hex digits in an install id (128 bits).
const ID_LENGTH: usize = 32;
/// Shown for a machine whose name cannot be read.
const UNKNOWN_MACHINE: &str = "unknown";

impl ClientIdentity {
    /// This install's id (from `id_file`, which is created when missing) and this machine's
    /// name. Reads and maybe writes a file: not for the UI thread.
    pub fn load_or_create(id_file: &Path) -> io::Result<Self> {
        Ok(Self {
            id: load_or_create_install_id(id_file)?,
            machine: machine_name(),
        })
    }

    /// An identity with an id of its own, for when the install's cannot be saved: replacing a
    /// lost connection then works until the app restarts.
    pub fn unsaved() -> Self {
        Self {
            id: new_install_id(),
            machine: machine_name(),
        }
    }
}

/// The install id saved in `path`, else a new one saved there. Several app instances may run
/// this at once: only a missing file is created, and one that holds no id is replaced. Two
/// instances creating it in the same instant may each keep their own id for this run.
fn load_or_create_install_id(path: &Path) -> io::Result<String> {
    if let Some(id) = read_install_id(path)? {
        return Ok(id);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let id = new_install_id();
    if create_install_id_file(path, &id)? {
        return Ok(id);
    }
    // Another instance created it just now, or it holds something else.
    if let Some(id) = read_install_id(path)? {
        return Ok(id);
    }
    fs::write(path, &id)?;
    Ok(id)
}

/// Write `id` to `path` when no file is there; false when one already is.
fn create_install_id_file(path: &Path, id: &str) -> io::Result<bool> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(id.as_bytes()).map(|()| true),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

fn read_install_id(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text.trim().to_owned()).filter(|id| is_install_id(id))),
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => Ok(None),
        Err(err) => Err(err),
    }
}

fn is_install_id(text: &str) -> bool {
    text.len() == ID_LENGTH
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A random id: not cryptographic, only unlikely to be another install's.
fn new_install_id() -> String {
    format!("{:016x}{:016x}", random_u64(), random_u64())
}

/// Each `RandomState` is seeded from the OS once per thread and then stepped, so two calls
/// differ; the time and process id separate installs further.
fn random_u64() -> u64 {
    let mut hasher = RandomState::new().build_hasher();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    hasher.write_u128(nanos);
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// This machine's name, as `agentws-server list` shows it.
fn machine_name() -> String {
    tidy_machine_name(host_name())
}

fn tidy_machine_name(raw: Option<String>) -> String {
    raw.map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| UNKNOWN_MACHINE.to_owned())
}

#[cfg(windows)]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is valid for its length; a name that fills it may be unterminated,
    // which the search for the end below allows.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

#[cfg(not(any(windows, unix)))]
fn host_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentws-identity-{name}-{}-{}",
            std::process::id(),
            random_u64()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_id_is_created_once_and_then_read_back() {
        let dir = temp_dir("stable");
        let path = dir.join("nested").join("client-id");
        let first = load_or_create_install_id(&path).unwrap();
        assert!(is_install_id(&first), "{first}");
        assert_eq!(load_or_create_install_id(&path).unwrap(), first);
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn install_id_tolerates_surrounding_whitespace_from_a_hand_edit() {
        let dir = temp_dir("whitespace");
        let path = dir.join("client-id");
        let id = "0123456789abcdef0123456789abcdef";
        fs::write(&path, format!("  {id}\r\n")).unwrap();
        assert_eq!(load_or_create_install_id(&path).unwrap(), id);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_holding_no_id_is_replaced_by_a_new_id() {
        let dir = temp_dir("garbage");
        let path = dir.join("client-id");
        for garbage in [
            &b"not an id"[..],
            b"",
            b"\xff\xfe",
            b"0123456789ABCDEF0123456789ABCDEF",
        ] {
            fs::write(&path, garbage).unwrap();
            let id = load_or_create_install_id(&path).unwrap();
            assert!(is_install_id(&id), "{id}");
            assert_eq!(fs::read_to_string(&path).unwrap(), id);
            assert_eq!(load_or_create_install_id(&path).unwrap(), id);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_install_ids_differ() {
        assert_ne!(new_install_id(), new_install_id());
    }

    #[test]
    fn machine_name_is_trimmed_and_never_empty() {
        assert_eq!(tidy_machine_name(Some(" box-1\n".into())), "box-1");
        assert_eq!(tidy_machine_name(Some("  ".into())), UNKNOWN_MACHINE);
        assert_eq!(tidy_machine_name(None), UNKNOWN_MACHINE);
        assert!(!machine_name().is_empty());
    }
}
