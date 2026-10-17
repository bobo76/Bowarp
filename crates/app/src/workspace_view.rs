use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agent_status::HookMessage;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, TitleBar, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    Keystroke, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, WeakEntity, Window, div, px,
};
use term_core::LaunchSpec;
use workspace_model::{Location, display_path};

use crate::hooks::ClaudeHooks;
use crate::launcher::Launcher;
use crate::sidebar::{self, SIDEBAR_WIDTH, TabDetail, TabRow};
use crate::terminal_view::{TabChanged, TerminalView};

struct Tab {
    view: Entity<TerminalView>,
    /// The launch before status hooks were attached, for opening another tab like it.
    spec: LaunchSpec,
    _changed: Subscription,
}

/// Window content: the title bar above the tab sidebar and the selected terminal.
pub struct WorkspaceView {
    tabs: Vec<Tab>,
    active: usize,
    sidebar_visible: bool,
    /// For shortening folders to `~` in the sidebar.
    home: Option<PathBuf>,
    next_session_id: u64,
    launchers: Rc<[Launcher]>,
    hooks: Option<Rc<ClaudeHooks>>,
    _hook_messages: Task<()>,
}

impl WorkspaceView {
    pub fn new(
        launchers: Rc<[Launcher]>,
        hooks: Option<Rc<ClaudeHooks>>,
        mut hook_messages: UnboundedReceiver<HookMessage>,
        cx: &mut Context<Self>,
    ) -> Self {
        let hook_task = cx.spawn(async move |this, cx| {
            while let Some(message) = hook_messages.next().await {
                if this
                    .update(cx, |workspace, cx| workspace.on_hook_message(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            tabs: Vec::new(),
            active: 0,
            sidebar_visible: true,
            home: std::env::home_dir(),
            next_session_id: 1,
            launchers,
            hooks,
            _hook_messages: hook_task,
        }
    }

    /// Start `spec` in a new tab and select it.
    pub fn open_tab(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<()> {
        let id = self.next_session_id;
        let mut launch = spec.clone();
        let reports_status = self
            .hooks
            .as_ref()
            .is_some_and(|hooks| hooks.attach(id, &mut launch));
        let view = TerminalView::spawn(id, &launch, reports_status, cx)?;
        self.next_session_id += 1;
        let changed = cx.subscribe(&view, |_, _, _: &TabChanged, cx| cx.notify());
        self.tabs.push(Tab {
            view,
            spec,
            _changed: changed,
        });
        self.activate(self.tabs.len() - 1, window, cx);
        Ok(())
    }

    fn open_tab_or_notify(
        &mut self,
        spec: LaunchSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let program = launch_name(&spec);
        if let Err(err) = self.open_tab(spec, window, cx) {
            log::error!("failed to start {program}: {err}");
            window.push_notification(
                Notification::error(format!("Failed to start {program}: {err}")),
                cx,
            );
        }
    }

    /// Folder for new tabs: the selected tab's current one, else the app's working directory.
    fn new_tab_cwd(&self, cx: &App) -> Option<PathBuf> {
        self.tabs
            .get(self.active)
            .and_then(|tab| {
                tab.view
                    .read(cx)
                    .location()
                    .map(|location| location.dir.clone())
            })
            .or_else(|| std::env::current_dir().ok())
    }

    fn open_launcher(&mut self, launcher: &Launcher, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.new_tab_cwd(cx);
        if let Some(spec) = launcher.spec(cwd) {
            self.open_tab_or_notify(spec, window, cx);
        }
    }

    /// Open another tab running what the selected tab runs.
    fn duplicate_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let spec = tab.spec.clone();
        self.open_tab_or_notify(spec, window, cx);
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let view = tab.view.clone();
        if let Some(previous) = self.tabs.get(self.active).filter(|_| self.active != index) {
            previous
                .view
                .update(cx, |view, cx| view.set_visible(false, cx));
        }
        self.active = index;
        view.update(cx, |view, cx| view.set_visible(true, cx));
        window.focus(&view.focus_handle(cx), cx);
        cx.notify();
    }

    /// Close a tab, ending its process. Closing the last tab closes the window.
    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        if index < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.activate(self.active, window, cx);
    }

    fn cycle(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count == 0 {
            return;
        }
        let next = if forward {
            (self.active + 1) % count
        } else {
            (self.active + count - 1) % count
        };
        self.activate(next, window, cx);
    }

    fn on_hook_message(&mut self, message: HookMessage, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.view.read(cx).id() == message.session)
        else {
            return;
        };
        tab.view.update(cx, |view, cx| {
            view.apply_hook_event(message.event, message.at, cx)
        });
    }

    /// Tab shortcuts run in the capture phase, before the terminal turns keys into input.
    fn on_key_down_capture(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.handle_tab_shortcut(&event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab cycle tabs, Ctrl+1..9 select one, Ctrl+Shift+T opens another
    /// tab like the selected one, Ctrl+Shift+W closes it, Ctrl+Shift+B shows or hides the
    /// sidebar.
    fn handle_tab_shortcut(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = &keystroke.modifiers;
        if !modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        match (keystroke.key.as_str(), modifiers.shift) {
            ("tab", shift) => self.cycle(!shift, window, cx),
            ("t", true) => self.duplicate_active(window, cx),
            ("w", true) => self.close_tab(self.active, window, cx),
            ("b", true) => self.toggle_sidebar(cx),
            (key, false) => match tab_number(key) {
                Some(number) => self.activate(number - 1, window, cx),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = cx.entity().downgrade();
        let new_tab_button = self.new_tab_button(workspace.clone());
        let rows = self.tabs.iter().enumerate().map(|(index, tab)| {
            let view = tab.view.read(cx);
            let row = TabRow {
                index,
                kind: view.kind(),
                name: view.name().to_string().into(),
                detail: view
                    .location()
                    .map(|location| tab_detail(location, self.home.as_deref())),
                status: view.tab_status(),
                selected: index == self.active,
            };
            let select = workspace.clone();
            let close = workspace.clone();
            sidebar::tab_row(
                row,
                move |_, window, cx| {
                    let _ =
                        select.update(cx, |workspace, cx| workspace.activate(index, window, cx));
                },
                move |_, window, cx| {
                    cx.stop_propagation();
                    let _ =
                        close.update(cx, |workspace, cx| workspace.close_tab(index, window, cx));
                },
                cx,
            )
        });
        let theme = cx.theme();
        div()
            .id("sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(34.))
                    .pl_3()
                    .pr_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("SESSIONS")
                    .child(new_tab_button),
            )
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_1()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (icon, tooltip) = if self.sidebar_visible {
            (IconName::PanelLeftClose, "Hide sidebar (Ctrl+Shift+B)")
        } else {
            (IconName::PanelLeftOpen, "Show sidebar (Ctrl+Shift+B)")
        };
        Button::new("toggle-sidebar")
            .icon(icon)
            .ghost()
            .small()
            .tooltip(tooltip)
            .on_click(cx.listener(|workspace, _, _, cx| workspace.toggle_sidebar(cx)))
    }

    fn new_tab_button(&self, workspace: WeakEntity<Self>) -> impl IntoElement {
        let launchers = self.launchers.clone();
        Button::new("new-tab")
            .icon(IconName::Plus)
            .ghost()
            .xsmall()
            .tooltip("New tab")
            .dropdown_menu(move |mut menu, _, _| {
                for launcher in launchers.iter() {
                    let available = launcher.program.is_some();
                    let label = if available {
                        launcher.label.clone()
                    } else {
                        format!("{} (not installed)", launcher.label)
                    };
                    let launcher = launcher.clone();
                    let workspace = workspace.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .icon(sidebar::kind_icon(launcher.kind))
                            .disabled(!available)
                            .on_click(move |_, window, cx| {
                                let _ = workspace.update(cx, |workspace, cx| {
                                    workspace.open_launcher(&launcher, window, cx)
                                });
                            }),
                    );
                }
                menu
            })
    }
}

/// The branch when the folder is in a repository, else the folder.
fn tab_detail(location: &Location, home: Option<&Path>) -> TabDetail {
    match &location.head {
        Some(head) => TabDetail::Branch(SharedString::from(head.name().to_string())),
        None => TabDetail::Folder(display_path(&location.dir, home).into()),
    }
}

/// The tab number for Ctrl+1..9.
fn tab_number(key: &str) -> Option<usize> {
    key.parse().ok().filter(|number| (1..=9).contains(number))
}

fn launch_name(spec: &LaunchSpec) -> String {
    spec.program.clone().unwrap_or_else(|| "shell".into())
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.tabs.get(self.active) {
            Some(tab) => tab.view.focus_handle(cx),
            None => cx.focus_handle(),
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tabs.get(self.active).map(|tab| tab.view.clone());
        let title = active.as_ref().map_or_else(
            || "Agent Workspace".into(),
            |view| view.read(cx).display_title(),
        );
        if window.window_title() != title {
            window.set_window_title(&title);
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .capture_key_down(cx.listener(Self::on_key_down_capture))
            .child(
                TitleBar::new().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.sidebar_toggle(cx))
                        .child(div().text_sm().child(title)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_visible, |content| {
                        content.child(self.render_sidebar(cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().children(active)),
            )
    }
}

#[cfg(test)]
mod tests {
    use workspace_model::GitHead;

    use super::*;

    #[test]
    fn detail_prefers_the_branch_over_the_folder() {
        let home = Path::new("/home/user");
        let in_repo = Location {
            dir: home.join("dev/app"),
            head: Some(GitHead::Branch("main".into())),
        };
        let plain = Location {
            dir: home.join(".config"),
            head: None,
        };

        assert!(matches!(
            tab_detail(&in_repo, Some(home)),
            TabDetail::Branch(branch) if branch == "main"
        ));
        assert!(matches!(
            tab_detail(&plain, Some(home)),
            TabDetail::Folder(folder) if folder == format!("~{}.config", std::path::MAIN_SEPARATOR)
        ));
    }

    #[test]
    fn tab_numbers() {
        assert_eq!(tab_number("1"), Some(1));
        assert_eq!(tab_number("9"), Some(9));
        assert_eq!(tab_number("0"), None);
        assert_eq!(tab_number("a"), None);
    }
}
