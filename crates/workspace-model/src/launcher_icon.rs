/// Icon the new-tab menu shows next to a launcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherIcon {
    /// A shell without an icon of its own (`sh`, `dash`, ...).
    Shell,
    /// PowerShell 7 (`pwsh`) and Windows PowerShell (`powershell`).
    PowerShell,
    CommandPrompt,
    GitBash,
    Bash,
    Zsh,
    Fish,
    Ubuntu,
    Debian,
    /// Any other WSL distro.
    Linux,
    Claude,
    Copilot,
}

/// Icon for a shell run as `program`: a file name or a path (`/` or `\` separated), with or
/// without `.exe`, any case. Git Bash is told apart by where it is found, not by its name, so
/// its launcher sets [`LauncherIcon::GitBash`] itself.
pub fn shell_icon(program: &str) -> LauncherIcon {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = match name.rsplit_once('.') {
        Some((stem, extension)) if extension.eq_ignore_ascii_case("exe") => stem,
        _ => name,
    };
    match stem.to_ascii_lowercase().as_str() {
        "pwsh" | "powershell" => LauncherIcon::PowerShell,
        "cmd" => LauncherIcon::CommandPrompt,
        "bash" => LauncherIcon::Bash,
        "zsh" => LauncherIcon::Zsh,
        "fish" => LauncherIcon::Fish,
        _ => LauncherIcon::Shell,
    }
}

/// Icon for a WSL distro, from its registered name (`Ubuntu-22.04`, `Debian`, ...).
pub fn distro_icon(distro: &str) -> LauncherIcon {
    let distro = distro.to_ascii_lowercase();
    if distro.starts_with("ubuntu") {
        LauncherIcon::Ubuntu
    } else if distro.starts_with("debian") {
        LauncherIcon::Debian
    } else {
        LauncherIcon::Linux
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_shells() {
        assert_eq!(
            shell_icon(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            LauncherIcon::PowerShell
        );
        assert_eq!(
            shell_icon(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            LauncherIcon::PowerShell
        );
        assert_eq!(
            shell_icon(r"C:\Windows\system32\CMD.EXE"),
            LauncherIcon::CommandPrompt
        );
    }

    #[test]
    fn unix_shells() {
        assert_eq!(shell_icon("/usr/bin/bash"), LauncherIcon::Bash);
        assert_eq!(shell_icon("/bin/zsh"), LauncherIcon::Zsh);
        assert_eq!(shell_icon("fish"), LauncherIcon::Fish);
        assert_eq!(shell_icon("/usr/bin/pwsh"), LauncherIcon::PowerShell);
        assert_eq!(shell_icon("/bin/sh"), LauncherIcon::Shell);
        assert_eq!(shell_icon("/usr/bin/dash"), LauncherIcon::Shell);
        assert_eq!(shell_icon("/opt/bin/zsh.static"), LauncherIcon::Shell);
    }

    #[test]
    fn wsl_distros() {
        assert_eq!(distro_icon("Ubuntu"), LauncherIcon::Ubuntu);
        assert_eq!(distro_icon("Ubuntu-22.04"), LauncherIcon::Ubuntu);
        assert_eq!(distro_icon("debian"), LauncherIcon::Debian);
        assert_eq!(distro_icon("wsl-vpnkit"), LauncherIcon::Linux);
        assert_eq!(distro_icon("openSUSE-Tumbleweed"), LauncherIcon::Linux);
    }
}
