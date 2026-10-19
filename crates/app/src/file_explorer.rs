//! The file explorer panel: the selected tab's folder as a tree. Folders are read when they
//! are expanded and re-read when a watcher on the expanded folders reports a change.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, EventEmitter, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, WeakEntity, Window, div, px, uniform_list,
};
use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use workspace_model::file_tree::{
    self, DirEntry, ExpandedFolders, FileKind, TreeRow, entry_folder, parent_folder, relative_path,
    validate_name,
};

use crate::assets::{
    FILE_ARCHIVE_ICON, FILE_CODE_ICON, FILE_DATA_ICON, FILE_IMAGE_ICON, FILES_ICON, FOLDER_UP_ICON,
    MARKDOWN_ICON,
};
use crate::launcher::Launcher;
use crate::sidebar;

pub const EXPLORER_WIDTH: f32 = 300.;
const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 14.;
/// Changes arriving within this time are applied together.
const CHANGE_BATCH: Duration = Duration::from_millis(100);

/// What the explorer asks of the workspace.
pub enum ExplorerEvent {
    /// The `×` was clicked.
    Close,
    /// Type `cd <folder>` in the selected tab.
    CdTo(PathBuf),
    /// Open a new tab in the folder: the agent, or the default shell when `None`.
    OpenTab(PathBuf, Option<Launcher>),
    /// A name editor closed: the keyboard goes back to the terminal.
    EditEnded,
    /// Show the file in the viewer.
    OpenFile(PathBuf),
}

/// What an entry's context menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryCommand {
    NewFile,
    CdTo,
    OpenInNewTab,
    /// Open the agent at this index of the explorer's agents.
    OpenInNewTabWith(usize),
    Reveal,
    Rename,
    Delete,
    CopyPath,
    CopyRelativePath,
}

/// A folder's entries, or why it could not be read.
type Listing = Result<Vec<DirEntry>, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum EditTarget {
    /// A new file in the folder.
    NewFile(PathBuf),
    Rename(PathBuf),
}

/// The name being typed for a new or renamed entry.
struct NameEdit {
    target: EditTarget,
    input: Entity<InputState>,
    _events: Subscription,
}

/// One row of the list: an entry, the new-file editor, or a folder's read error.
enum ListRow {
    Entry(TreeRow),
    NewFile { depth: usize },
    Error { depth: usize, message: String },
}

pub struct FileExplorer {
    root: Option<PathBuf>,
    /// Whether the selected tab runs a shell, which "cd to directory" needs.
    can_cd: bool,
    /// The agent CLIs "Open in new tab with" offers.
    agents: Rc<[Launcher]>,
    expanded: ExpandedFolders,
    listings: HashMap<PathBuf, Listing>,
    selected: Option<PathBuf>,
    editing: Option<NameEdit>,
    /// `None` when the platform watcher could not start: folders then refresh only when
    /// expanded again or changed from the explorer.
    watcher: Option<RecommendedWatcher>,
    /// The watched folders, each by its real path (what the watcher reports), with the
    /// expanded folders showing it: a folder and a symlink to it share one watch.
    watched: HashMap<PathBuf, Vec<PathBuf>>,
    /// The real path of each open folder, so it is not looked up on every refresh.
    real_paths: HashMap<PathBuf, PathBuf>,
    rows: Rc<[ListRow]>,
    _changes: Task<()>,
}

impl EventEmitter<ExplorerEvent> for FileExplorer {}

impl FileExplorer {
    pub fn new(agents: Vec<Launcher>, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = mpsc::unbounded();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                return;
            };
            // Only entries appearing, going or being renamed change a listing; reading a
            // folder opens it, which would otherwise report a change and read it again.
            if matches!(
                event.kind,
                EventKind::Access(_)
                    | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
            ) {
                return;
            }
            for path in event.paths {
                let _ = sender.unbounded_send(path);
            }
        })
        .inspect_err(|err| log::warn!("file explorer: no folder watcher: {err}"))
        .ok();
        Self {
            root: None,
            can_cd: false,
            agents: agents.into(),
            expanded: ExpandedFolders::default(),
            listings: HashMap::new(),
            selected: None,
            editing: None,
            watcher,
            watched: HashMap::new(),
            real_paths: HashMap::new(),
            rows: Rc::new([]),
            _changes: Self::apply_changes(receiver, cx),
        }
    }

    /// Re-read the watched folders that changed, a batch of changes at a time.
    fn apply_changes(mut changes: UnboundedReceiver<PathBuf>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Some(first) = changes.next().await {
                cx.background_executor().timer(CHANGE_BATCH).await;
                let mut paths = vec![first];
                while let Ok(path) = changes.try_recv() {
                    paths.push(path);
                }
                if this
                    .update(cx, |explorer, cx| explorer.on_changes(paths, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn on_changes(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let folders = file_tree::folders_to_reread(&paths, &self.watched);
        if folders.is_empty() {
            return;
        }
        // A watched folder reported about itself was removed or replaced, and the watcher
        // drops its watch then: forget it so the refresh below watches it again.
        for path in &paths {
            if self.watched.remove(path).is_some() {
                self.real_paths.retain(|_, real| real != path);
            }
        }
        for folder in folders {
            self.listings.insert(folder.clone(), read_folder(&folder));
        }
        self.refresh(cx);
    }

    /// Show `root` (the selected tab's folder). `can_cd`: whether that tab runs a shell.
    pub fn set_root(&mut self, root: Option<PathBuf>, can_cd: bool, cx: &mut Context<Self>) {
        if can_cd != self.can_cd {
            self.can_cd = can_cd;
            cx.notify();
        }
        if root == self.root {
            return;
        }
        self.root = root;
        if self.editing.take().is_some() {
            cx.emit(ExplorerEvent::EditEnded);
        }
        // Folders left unwatched may have changed meanwhile.
        self.listings.clear();
        self.refresh(cx);
    }

    /// Show `folder` in the tree, expanded and selected, with the folders down to it
    /// expanded. `false` when it is not under the root shown.
    pub fn reveal(&mut self, folder: &Path, cx: &mut Context<Self>) -> bool {
        let Some(root) = self.root.clone() else {
            return false;
        };
        // Both real paths, as a link may hold `..` and the tab's folder a symlink.
        let (Ok(real_folder), Ok(real_root)) = (fs::canonicalize(folder), fs::canonicalize(&root))
        else {
            return false;
        };
        let Ok(relative) = real_folder.strip_prefix(&real_root) else {
            return false;
        };
        let mut current = root.clone();
        for part in relative.components() {
            current.push(part);
            self.expanded.expand(&root, &current);
        }
        self.selected = Some(current);
        self.refresh(cx);
        true
    }

    /// Read the expanded folders not read yet, watch exactly the expanded ones, and rebuild
    /// the rows.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            self.rows = Rc::new([]);
            self.sync_watches(HashSet::new());
            cx.notify();
            return;
        };
        let rows = loop {
            let rows = self.tree_rows(&root);
            let unread: Vec<PathBuf> = rows
                .iter()
                .filter(|row| row.expanded && !self.listings.contains_key(&row.path))
                .map(|row| row.path.clone())
                .collect();
            if unread.is_empty() {
                break rows;
            }
            for folder in unread {
                let listing = read_folder(&folder);
                self.listings.insert(folder, listing);
            }
        };
        let open: HashSet<PathBuf> = rows
            .iter()
            .filter(|row| row.expanded)
            .map(|row| row.path.clone())
            .collect();
        // Unwatched folders may change: read them again when they are shown again.
        self.listings.retain(|folder, _| open.contains(folder));
        self.rows = self.list_rows(rows).into();
        self.sync_watches(open);
        cx.notify();
    }

    fn tree_rows(&mut self, root: &Path) -> Vec<TreeRow> {
        let listings = &self.listings;
        file_tree::visible_rows(root, self.expanded.of(root), |folder| {
            listings
                .get(folder)
                .and_then(|listing| listing.as_ref().ok())
                .map(Vec::as_slice)
        })
    }

    /// The tree rows with the new-file editor and read errors placed under their folder.
    fn list_rows(&self, rows: Vec<TreeRow>) -> Vec<ListRow> {
        let new_file_in = match &self.editing {
            Some(NameEdit {
                target: EditTarget::NewFile(folder),
                ..
            }) => Some(folder.clone()),
            _ => None,
        };
        let mut list = Vec::with_capacity(rows.len() + 1);
        for row in rows {
            let depth = row.depth + 1;
            let error = row
                .expanded
                .then(|| self.listings.get(&row.path))
                .flatten()
                .and_then(|listing| listing.as_ref().err().cloned());
            let new_file = row.expanded && new_file_in.as_ref() == Some(&row.path);
            list.push(ListRow::Entry(row));
            if new_file {
                list.push(ListRow::NewFile { depth });
            }
            if let Some(message) = error {
                list.push(ListRow::Error { depth, message });
            }
        }
        list
    }

    fn sync_watches(&mut self, open: HashSet<PathBuf>) {
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        self.real_paths.retain(|folder, _| open.contains(folder));
        let mut watched: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for folder in open {
            let real = match self.real_paths.get(&folder) {
                Some(real) => real.clone(),
                None => match fs::canonicalize(&folder) {
                    Ok(real) => {
                        self.real_paths.insert(folder.clone(), real.clone());
                        real
                    }
                    // Not there (yet): look again next time.
                    Err(_) => folder.clone(),
                },
            };
            watched.entry(real).or_default().push(folder);
        }
        for real in self.watched.keys() {
            if !watched.contains_key(real) {
                let _ = watcher.unwatch(real);
            }
        }
        // Only folders whose watch holds are recorded, so a failed one is tried again.
        watched.retain(|real, _| {
            self.watched.contains_key(real)
                || watcher
                    .watch(real, RecursiveMode::NonRecursive)
                    .inspect_err(|err| {
                        log::debug!("file explorer: not watching {}: {err}", real.display())
                    })
                    .is_ok()
        });
        self.watched = watched;
    }

    fn click(&mut self, path: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        if is_dir && let Some(root) = self.root.clone() {
            self.expanded.toggle(&root, &path);
            self.selected = Some(path);
            self.refresh(cx);
            return;
        }
        self.selected = Some(path.clone());
        cx.emit(ExplorerEvent::OpenFile(path));
        cx.notify();
    }

    fn run_command(
        &mut self,
        command: EntryCommand,
        path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some(path.clone());
        let folder = entry_folder(&path, is_dir).to_path_buf();
        match command {
            EntryCommand::NewFile => self.start_edit(EditTarget::NewFile(folder), window, cx),
            EntryCommand::CdTo => cx.emit(ExplorerEvent::CdTo(folder)),
            EntryCommand::OpenInNewTab => cx.emit(ExplorerEvent::OpenTab(folder, None)),
            EntryCommand::OpenInNewTabWith(agent) => {
                if let Some(agent) = self.agents.get(agent) {
                    cx.emit(ExplorerEvent::OpenTab(folder, Some(agent.clone())));
                }
            }
            EntryCommand::Reveal => cx.reveal_path(&path),
            EntryCommand::Rename => self.start_edit(EditTarget::Rename(path), window, cx),
            EntryCommand::Delete => self.confirm_delete(path, is_dir, window, cx),
            EntryCommand::CopyPath => copy_text(path.display().to_string(), cx),
            EntryCommand::CopyRelativePath => {
                if let Some(root) = &self.root {
                    copy_text(relative_path(root, &path), cx);
                }
            }
        }
        cx.notify();
    }

    /// Edit a name in the tree: Enter or clicking elsewhere applies it, Escape cancels.
    fn start_edit(&mut self, target: EditTarget, window: &mut Window, cx: &mut Context<Self>) {
        let name = match &target {
            EditTarget::NewFile(folder) => {
                if let Some(root) = self.root.clone() {
                    self.expanded.expand(&root, folder);
                }
                String::new()
            }
            EditTarget::Rename(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("File name")
                .default_value(name)
        });
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        let events = cx.subscribe_in(&input, window, |explorer, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                explorer.commit_edit(window, cx);
            }
        });
        self.editing = Some(NameEdit {
            target,
            input,
            _events: events,
        });
        self.refresh(cx);
    }

    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else {
            return;
        };
        let typed = edit.input.read(cx).value();
        let result = match validate_name(&typed) {
            Ok(name) => match &edit.target {
                EditTarget::NewFile(folder) => create_file(folder, name),
                EditTarget::Rename(path) => self.rename(path, name),
            },
            // Leaving a new file's name empty cancels it.
            Err(file_tree::NameError::Empty) => Ok(None),
            Err(err) => Err(err.to_string()),
        };
        match result {
            Ok(Some(path)) => {
                if let Some(folder) = path.parent() {
                    self.listings
                        .insert(folder.to_path_buf(), read_folder(folder));
                }
                self.selected = Some(path);
            }
            Ok(None) => {}
            Err(message) => window.push_notification(Notification::error(message), cx),
        }
        self.end_edit(cx);
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.end_edit(cx);
        }
    }

    fn end_edit(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        cx.emit(ExplorerEvent::EditEnded);
    }

    /// Rename `path` to `name` in its folder, never replacing an existing entry (only itself,
    /// for a change of case on a case-insensitive file system). Returns the new path, or
    /// `None` when the name did not change.
    fn rename(&mut self, path: &Path, name: &str) -> Result<Option<PathBuf>, String> {
        let target = path.with_file_name(name);
        if target == path {
            return Ok(None);
        }
        if fs::symlink_metadata(&target).is_ok() && !same_entry(path, &target) {
            return Err(format!("\"{name}\" already exists"));
        }
        fs::rename(path, &target).map_err(|err| format!("Couldn't rename: {err}"))?;
        self.expanded.rename(path, &target);
        Ok(Some(target))
    }

    fn confirm_delete(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let description = if is_dir {
            "The folder and everything in it are deleted permanently, not moved to the trash."
        } else {
            "The file is deleted permanently, not moved to the trash."
        };
        let explorer = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let explorer = explorer.clone();
            let path = path.clone();
            alert
                .title(format!("Delete \"{name}\"?"))
                .description(description)
                .ok_text("Delete")
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let path = path.clone();
                    let _ = explorer.update(cx, |explorer, cx| explorer.delete(path, window, cx));
                    true
                })
        });
    }

    /// Delete `path` for good off the UI thread (a large folder takes seconds), then show the
    /// folder that held it as it is.
    fn delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let removed = path.clone();
            let result = cx
                .background_spawn(async move { remove_entry(&removed) })
                .await;
            let _ = this.update_in(cx, |explorer, window, cx| {
                if let Err(err) = result {
                    window.push_notification(
                        Notification::error(format!("Couldn't delete: {}", describe(&err))),
                        cx,
                    );
                }
                if let Some(folder) = path.parent()
                    && explorer.listings.contains_key(folder)
                {
                    explorer
                        .listings
                        .insert(folder.to_path_buf(), read_folder(folder));
                }
                if explorer.selected.as_deref() == Some(path.as_path()) {
                    explorer.selected = None;
                }
                explorer.refresh(cx);
            });
        })
        .detach();
    }

    fn entry_menu(
        explorer: WeakEntity<Self>,
        path: PathBuf,
        is_dir: bool,
        is_root: bool,
        can_cd: bool,
        agents: Rc<[Launcher]>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, window, cx| {
            let item = |label: &'static str, command: EntryCommand, enabled: bool| {
                command_item(&explorer, &path, is_dir, label.into(), command, enabled)
            };
            let (submenu_explorer, submenu_path) = (explorer.clone(), path.clone());
            let agents = agents.clone();
            menu.min_w(px(200.))
                .item(item("New file", EntryCommand::NewFile, true))
                .separator()
                .item(item("cd to directory", EntryCommand::CdTo, can_cd))
                .item(item("Open in new tab", EntryCommand::OpenInNewTab, true))
                .submenu("Open in new tab with", window, cx, move |menu, _, _| {
                    agents
                        .iter()
                        .enumerate()
                        .fold(menu, |menu, (index, agent)| {
                            let installed = agent.program.is_some();
                            let label = if installed {
                                agent.label.clone()
                            } else {
                                format!("{} (not installed)", agent.label)
                            };
                            menu.item(
                                command_item(
                                    &submenu_explorer,
                                    &submenu_path,
                                    is_dir,
                                    label.into(),
                                    EntryCommand::OpenInNewTabWith(index),
                                    installed,
                                )
                                .icon(sidebar::launcher_icon(agent.icon)),
                            )
                        })
                })
                .item(item("Reveal in file manager", EntryCommand::Reveal, true))
                .item(item("Rename", EntryCommand::Rename, !is_root))
                .item(item("Delete", EntryCommand::Delete, !is_root))
                .separator()
                .item(item("Copy path", EntryCommand::CopyPath, true))
                .item(item(
                    "Copy relative path",
                    EntryCommand::CopyRelativePath,
                    true,
                ))
        }
    }

    fn render_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let indent = |depth: usize| px(6. + depth as f32 * INDENT);
        match &self.rows[index] {
            ListRow::Error { depth, message } => div()
                .h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .pl(indent(*depth) + px(INDENT + 4.))
                .text_xs()
                .text_color(muted)
                .child(format!("Can't read: {message}"))
                .into_any_element(),
            ListRow::NewFile { depth } => self
                .row_shell(*depth, cx)
                .child(div().w(px(INDENT)).flex_none())
                .child(Icon::new(IconName::File).small().text_color(muted))
                .children(self.name_editor(cx))
                .into_any_element(),
            ListRow::Entry(row) => {
                let path = row.path.clone();
                let is_dir = row.is_dir;
                let selected = self.selected.as_ref() == Some(&row.path);
                let renaming = matches!(
                    &self.editing,
                    Some(NameEdit { target: EditTarget::Rename(target), .. }) if *target == row.path
                );
                let hovered = sidebar::hover_background(cx);
                let menu = Self::entry_menu(
                    cx.entity().downgrade(),
                    row.path.clone(),
                    is_dir,
                    row.depth == 0,
                    self.can_cd,
                    self.agents.clone(),
                );
                let chevron = if !is_dir {
                    div().w(px(INDENT)).flex_none().into_any_element()
                } else {
                    let icon = if row.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    };
                    Icon::new(icon).small().text_color(muted).into_any_element()
                };
                let theme = cx.theme();
                self.row_shell(row.depth, cx)
                    .id(("explorer-row", index))
                    .when(selected, |row| row.bg(theme.sidebar_accent))
                    .when(!selected, |row| row.hover(|style| style.bg(hovered)))
                    .on_click(cx.listener(move |explorer, _, _, cx| {
                        explorer.click(path.clone(), is_dir, cx)
                    }))
                    .child(chevron)
                    .child(entry_icon(row, cx))
                    .map(|line| {
                        if renaming {
                            line.children(self.name_editor(cx))
                        } else {
                            line.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(row.name.clone()),
                            )
                        }
                    })
                    .context_menu(menu)
                    .into_any_element()
            }
        }
    }

    fn row_shell(&self, depth: usize, cx: &App) -> gpui_kit::Div {
        div()
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap_1p5()
            .pl(px(6. + depth as f32 * INDENT))
            .pr_2()
            .text_sm()
            .text_color(cx.theme().sidebar_foreground)
    }

    fn name_editor(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let edit = self.editing.as_ref()?;
        let editor = div()
            .flex_1()
            .min_w_0()
            // Keep clicks inside the editor from reaching the row.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(|explorer, _: &Escape, _, cx| explorer.cancel_edit(cx)))
            .child(Input::new(&edit.input).xsmall());
        Some(editor.into_any_element())
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(40.))
            .pl_2()
            .pr_1()
            .child(
                div()
                    .id("explorer-files-view")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(26.))
                    .rounded_md()
                    .bg(theme.sidebar_accent)
                    .child(Icon::empty().path(FILES_ICON).small()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.parent_button(cx))
                    .child(
                        Button::new("close-explorer")
                            .icon(IconName::Close)
                            .ghost()
                            .xsmall()
                            .tooltip("Close explorer")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ExplorerEvent::Close))),
                    ),
            )
    }

    /// "Parent folder (cd ..)": types `cd` to the folder above the tree's in a shell tab, and
    /// the tree follows the shell there. Disabled at a filesystem or drive root, and in other
    /// tabs, whose tree always shows the session's folder.
    fn parent_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let parent = self
            .root
            .as_deref()
            .and_then(parent_folder)
            .map(Path::to_path_buf)
            .filter(|_| self.can_cd);
        Button::new("explorer-parent")
            .icon(Icon::empty().path(FOLDER_UP_ICON))
            .ghost()
            .xsmall()
            .disabled(parent.is_none())
            .tooltip("Parent folder (cd ..)")
            .on_click(cx.listener(move |_, _, _, cx| {
                if let Some(parent) = parent.clone() {
                    cx.emit(ExplorerEvent::CdTo(parent));
                }
            }))
    }
}

impl Render for FileExplorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border, muted) =
            (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
        let body = if self.root.is_none() {
            div()
                .p_3()
                .text_sm()
                .text_color(muted)
                .child("No folder")
                .into_any_element()
        } else {
            uniform_list(
                "explorer-rows",
                self.rows.len(),
                cx.processor(|explorer, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|index| explorer.render_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element()
        };
        div()
            .id("file-explorer")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(EXPLORER_WIDTH))
            .h_full()
            .bg(background)
            .border_r_1()
            .border_color(border)
            .child(self.render_header(cx))
            .child(body)
    }
}

/// A context menu entry running `command` on the entry at `path`.
fn command_item(
    explorer: &WeakEntity<FileExplorer>,
    path: &Path,
    is_dir: bool,
    label: SharedString,
    command: EntryCommand,
    enabled: bool,
) -> PopupMenuItem {
    let (explorer, path) = (explorer.clone(), path.to_path_buf());
    PopupMenuItem::new(label)
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            let _ = explorer.update(cx, |explorer, cx| {
                explorer.run_command(command, path.clone(), is_dir, window, cx)
            });
        })
}

/// The icon for a row: an open or closed folder, or the file's kind.
fn entry_icon(row: &TreeRow, cx: &App) -> Icon {
    let muted = cx.theme().muted_foreground;
    let icon = if row.is_dir {
        Icon::new(if row.expanded {
            IconName::FolderOpen
        } else {
            IconName::Folder
        })
    } else {
        match file_tree::file_kind(&row.name) {
            FileKind::Markdown => Icon::empty().path(MARKDOWN_ICON),
            FileKind::Image => Icon::empty().path(FILE_IMAGE_ICON),
            FileKind::Code => Icon::empty().path(FILE_CODE_ICON),
            FileKind::Data => Icon::empty().path(FILE_DATA_ICON),
            FileKind::Archive => Icon::empty().path(FILE_ARCHIVE_ICON),
            FileKind::Text => Icon::new(IconName::FileText),
            FileKind::Other => Icon::new(IconName::File),
        }
    };
    icon.small().text_color(muted)
}

/// A folder's entries, sorted. Symlinks to folders count as folders; an entry whose type
/// can't be read counts as a file.
fn read_folder(folder: &Path) -> Listing {
    let entries = fs::read_dir(folder).map_err(|err| describe(&err))?;
    let mut listing: Vec<DirEntry> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let is_dir = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
                Ok(kind) => kind.is_dir(),
                Err(_) => false,
            };
            DirEntry::new(entry.file_name().to_string_lossy(), is_dir)
        })
        .collect();
    file_tree::sort_entries(&mut listing);
    Ok(listing)
}

fn describe(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        io::ErrorKind::NotFound => "not found".to_string(),
        _ => err.to_string(),
    }
}

/// Create an empty file `name` in `folder`, never replacing an existing one.
fn create_file(folder: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let path = folder.join(name);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| match err.kind() {
            io::ErrorKind::AlreadyExists => format!("\"{name}\" already exists"),
            _ => format!("Couldn't create \"{name}\": {err}"),
        })?;
    Ok(Some(path))
}

/// Remove `path`: a folder with its contents, a symlink itself (not its target).
fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(err) => Err(err),
    }
}

/// Whether `a` and `b` name the same entry, as names differing only in case do on a
/// case-insensitive file system.
fn same_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
    }
}

fn copy_text(text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}
