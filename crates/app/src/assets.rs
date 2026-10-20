use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

pub const GIT_BRANCH_ICON: &str = "icons/agentws/git-branch.svg";
pub const AGENT_SPARK_ICON: &str = "icons/agentws/agent-spark.svg";
pub const PIN_ICON: &str = "icons/agentws/pin.svg";
pub const PROMPT_ICON: &str = "icons/agentws/terminal.svg";
pub const POWERSHELL_ICON: &str = "icons/agentws/square-chevron-right.svg";
pub const GIT_ICON: &str = "icons/agentws/git.svg";
pub const BASH_ICON: &str = "icons/agentws/gnubash.svg";
pub const ZSH_ICON: &str = "icons/agentws/zsh.svg";
pub const FISH_ICON: &str = "icons/agentws/fishshell.svg";
pub const UBUNTU_ICON: &str = "icons/agentws/ubuntu.svg";
pub const DEBIAN_ICON: &str = "icons/agentws/debian.svg";
pub const LINUX_ICON: &str = "icons/agentws/linux.svg";
pub const COPILOT_ICON: &str = "icons/agentws/githubcopilot.svg";
pub const FILES_ICON: &str = "icons/agentws/files.svg";
pub const FOLDER_UP_ICON: &str = "icons/agentws/folder-up.svg";
pub const FILE_IMAGE_ICON: &str = "icons/agentws/file-image.svg";
pub const FILE_CODE_ICON: &str = "icons/agentws/file-code.svg";
pub const FILE_DATA_ICON: &str = "icons/agentws/file-braces.svg";
pub const FILE_ARCHIVE_ICON: &str = "icons/agentws/file-archive.svg";
pub const MARKDOWN_ICON: &str = "icons/agentws/markdown.svg";
pub const CODE_ICON: &str = "icons/agentws/code-xml.svg";
pub const APP_ICON: &str = "icons/agentws/app-icon.svg";

/// Icons bundled with the app: (asset path, SVG). `git-branch`, `pin`, `terminal`,
/// `square-chevron-right`, `files`, `folder-up`, `code-xml` and the `file-*` icons are from
/// Lucide (ISC, see `assets/icons/LICENSE-LUCIDE`); `git`, `gnubash`, `zsh`, `fishshell`,
/// `ubuntu`, `debian`, `linux`, `githubcopilot` and `markdown` are from Simple Icons 15.0.0
/// (CC0, see `assets/icons/LICENSE-SIMPLE-ICONS`); the spark and the app icon are our own.
const ICONS: &[(&str, &[u8])] = &[
    (
        GIT_BRANCH_ICON,
        include_bytes!("../assets/icons/git-branch.svg"),
    ),
    (
        AGENT_SPARK_ICON,
        include_bytes!("../assets/icons/agent-spark.svg"),
    ),
    (PIN_ICON, include_bytes!("../assets/icons/pin.svg")),
    (PROMPT_ICON, include_bytes!("../assets/icons/terminal.svg")),
    (
        POWERSHELL_ICON,
        include_bytes!("../assets/icons/square-chevron-right.svg"),
    ),
    (GIT_ICON, include_bytes!("../assets/icons/git.svg")),
    (BASH_ICON, include_bytes!("../assets/icons/gnubash.svg")),
    (ZSH_ICON, include_bytes!("../assets/icons/zsh.svg")),
    (FISH_ICON, include_bytes!("../assets/icons/fishshell.svg")),
    (UBUNTU_ICON, include_bytes!("../assets/icons/ubuntu.svg")),
    (DEBIAN_ICON, include_bytes!("../assets/icons/debian.svg")),
    (LINUX_ICON, include_bytes!("../assets/icons/linux.svg")),
    (
        COPILOT_ICON,
        include_bytes!("../assets/icons/githubcopilot.svg"),
    ),
    (FILES_ICON, include_bytes!("../assets/icons/files.svg")),
    (
        FOLDER_UP_ICON,
        include_bytes!("../assets/icons/folder-up.svg"),
    ),
    (
        FILE_IMAGE_ICON,
        include_bytes!("../assets/icons/file-image.svg"),
    ),
    (
        FILE_CODE_ICON,
        include_bytes!("../assets/icons/file-code.svg"),
    ),
    (
        FILE_DATA_ICON,
        include_bytes!("../assets/icons/file-braces.svg"),
    ),
    (
        FILE_ARCHIVE_ICON,
        include_bytes!("../assets/icons/file-archive.svg"),
    ),
    (
        MARKDOWN_ICON,
        include_bytes!("../assets/icons/markdown.svg"),
    ),
    (CODE_ICON, include_bytes!("../assets/icons/code-xml.svg")),
    (
        APP_ICON,
        include_bytes!("../assets/app-icon/agent-workspace.svg"),
    ),
];

/// The app's own icons in front of the component library's.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ICONS.iter().find(|(icon, _)| *icon == path) {
            Some((_, svg)) => Ok(Some(Cow::Borrowed(svg))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(
            ICONS
                .iter()
                .filter(|(icon, _)| icon.starts_with(path))
                .map(|(icon, _)| SharedString::from(*icon)),
        );
        Ok(paths)
    }
}
