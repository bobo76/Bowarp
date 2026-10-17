use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

/// Icons bundled with the app: (asset path, SVG). `git-branch.svg` is from Lucide (ISC, see
/// `assets/icons/LICENSE-LUCIDE`); the spark is our own.
const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/agentws/git-branch.svg",
        include_bytes!("../assets/icons/git-branch.svg"),
    ),
    (
        "icons/agentws/agent-spark.svg",
        include_bytes!("../assets/icons/agent-spark.svg"),
    ),
];

pub const GIT_BRANCH_ICON: &str = ICONS[0].0;
pub const AGENT_SPARK_ICON: &str = ICONS[1].0;

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
