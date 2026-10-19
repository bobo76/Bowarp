//! The About dialog: app icon, name, version (with the commit it was built from) and a link to
//! the repository.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::link::Link;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, WindowExt};
use gpui_kit::{
    App, ClipboardItem, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window, div,
    img, px,
};
use workspace_model::version_line;

use crate::assets::APP_ICON;

pub const APP_NAME: &str = "Agent Workspace";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The line a bug report quotes: "agent-workspace 0.1.0 (abc1234)".
fn version() -> String {
    version_line(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        option_env!("AGENTWS_GIT_COMMIT"),
    )
}

/// The title bar button that opens the About dialog.
pub fn about_button() -> impl IntoElement {
    Button::new("about")
        .icon(IconName::Info)
        .ghost()
        .small()
        .tooltip("About Agent Workspace")
        .on_click(|_, window, cx| open(window, cx))
}

/// Opens the About dialog; Escape, the × or Close dismiss it.
pub fn open(window: &mut Window, cx: &mut App) {
    let version = SharedString::from(version());
    window.open_dialog(cx, move |dialog, _, cx| {
        let copied = version.clone();
        dialog
            .w(px(380.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(img(APP_ICON).size(px(72.)))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(APP_NAME),
                    )
                    // Selectable, so part of it can be copied too; Copy takes all of it.
                    .child(
                        TextView::markdown("about-version", version.clone())
                            .selectable(true)
                            .text_sm()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        Link::new("about-repository")
                            .href(REPOSITORY)
                            .text_sm()
                            .child(REPOSITORY),
                    ),
            )
            .footer(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("about-copy")
                            .icon(IconName::Copy)
                            .label("Copy version")
                            .outline()
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    copied.to_string(),
                                ));
                            }),
                    )
                    .child(
                        Button::new("about-close")
                            .label("Close")
                            .primary()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}
