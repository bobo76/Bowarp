/// Platform policies that must also be testable on other hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
    Other,
}

impl Platform {
    pub const CURRENT: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::MacOs
    } else if cfg!(target_os = "linux") {
        Self::Linux
    } else {
        Self::Other
    };
}
