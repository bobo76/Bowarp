//! The Git page: a repository the tabs are in (picked from a menu), its changed files in a
//! "Staged Changes" and a "Changes" group, and staging or unstaging them whole. Every `git`
//! runs off the UI thread, one at a time, so two clicks never race for the index lock. While
//! the page shows, the repository's git folder, its top folder and the folders of its changed
//! files are watched, and a change reads the status again; nothing is watched while hidden.
//! The selected file's diff shows on the right ([`diff_view`]), read with the status so the
//! two always match. Its changed lines can be selected and staged or unstaged alone
//! ([`line_staging`]). What is staged is committed from the box above the lists
//! ([`commit_box`]).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, FontWeight, Hsla,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, ParentElement, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    UniformListScrollHandle, Window, actions, div, px, uniform_list,
};
use notify::event::ModifyKind;
use notify::{Event, EventKind};
use workspace_model::git::{
    self, Change, DiffSide, FileDiff, FileStatus, Head, LineSelection, MAX_STATUS_FILES, RepoEntry,
    RepoStatus, StatusRow,
};
use workspace_model::layout::Direction;

use self::commit_box::{COMMIT_CONTEXT, CommitDone};
use self::diff_view::ShownDiff;

use crate::assets::GIT_BRANCH_ICON;
use crate::folder_watcher::{FolderWatcher, WatchesFolders};
use crate::sidebar;

/// Height of a row of the list of changes.
const ROW_HEIGHT: f32 = 26.;
/// Width of the list of changes.
const LIST_WIDTH: f32 = 340.;
/// The widest a file's name gets in its row (its folder takes the rest).
const FILE_NAME_MAX_WIDTH: f32 = LIST_WIDTH * 0.6;
/// Width of a file row's status letter.
const CHANGE_LETTER_WIDTH: f32 = 12.;
/// Height of the page's header.
const HEADER_HEIGHT: f32 = 40.;
/// At most this many folders of changed files are watched (besides the top and git folders).
const MAX_WATCHED_FOLDERS: usize = 256;
/// The files in the git folder whose change means the status changed.
const GIT_DIR_FILES: [&str; 2] = ["index", "HEAD"];
/// Why a selected file's diff isn't read: it left its group before the read.
const NO_LONGER_CHANGED: &str = "The file has no change here anymore.";

/// The page's key context: its keys win over the window's Tab focus cycling.
const KEY_CONTEXT: &str = "GitPage";

actions!(
    git_page,
    [
        /// Tab: move the keyboard on, from the commit message to the list to the diff.
        FocusNext,
        /// Shift+Tab: move the keyboard back, from the diff to the list to the message.
        FocusPrevious,
        /// Ctrl+Enter (Cmd+Enter) in the commit message: commit what is staged.
        Commit
    ]
);

pub fn init(cx: &mut App) {
    // In the message, Tab moves on rather than indenting, and Ctrl+Enter commits rather than
    // adding a line (bound later than the input's own keys, at its depth, so these win).
    let message = format!("{COMMIT_CONTEXT} > Input");
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("tab", FocusNext, Some(&message)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(&message)),
        KeyBinding::new("secondary-enter", Commit, Some(&message)),
    ]);
}

/// Something to do with `git`, in the order asked.
enum Op {
    /// List the repositories of these folders (the selected tab's first) and read the status
    /// of the chosen one: the repository of `selected` (the selected tab's folder) when given
    /// and in one, else the one shown when still listed, else the first.
    FindRepos {
        folders: Vec<PathBuf>,
        selected: Option<PathBuf>,
    },
    /// Show this repository.
    Choose(RepoEntry),
    Refresh,
    /// Change the index of the repository shown when it was asked (another may show by the
    /// time it runs), then read the status of the one shown.
    Change(Target, IndexChange),
    /// Read the selected file's diff only (not the status).
    ReadDiff,
    /// Commit what is staged in the repository shown when it was asked, with this message.
    Commit(Target, String),
}

/// The repository an [`Op::Change`] is for, as the page showed it when it was asked.
struct Target {
    root: PathBuf,
    /// What its HEAD pointed at (unstaging works differently before the first commit).
    head: Head,
}

/// How an [`Op::Change`] changes the index.
enum IndexChange {
    Stage(Vec<FileStatus>),
    Unstage(Vec<FileStatus>),
    StageAll,
    UnstageAll,
    /// A [`git::partial_patch`] of the diff on `side`: stages its lines (Unstaged) or unstages
    /// them (Staged).
    Apply {
        side: DiffSide,
        patch: Vec<u8>,
    },
}

/// The selected file when an op starts.
struct Selection {
    side: DiffSide,
    path: String,
    /// Its status before the op (what [`Op::ReadDiff`] reads the diff of).
    file: Option<FileStatus>,
}

/// What an [`Op`] gave, read off the UI thread.
struct Outcome {
    /// The new list of repositories, when the op listed them.
    repos: Option<Vec<RepoEntry>>,
    /// Why the op's action failed.
    error: Option<String>,
    /// The repository and its status, unless the op read only a diff.
    status: Option<StatusRead>,
    /// The diff of the file selected when the op started, if it still had a change there.
    diff: Option<DiffRead>,
    /// The commit the op made or tried.
    commit: Option<CommitDone>,
}

struct StatusRead {
    repo: Option<RepoEntry>,
    /// The repository's status after the op, `None` without a repository.
    status: Option<Result<RepoStatus, String>>,
    /// The folders to watch for this status ([`watched_folders`], which reads the disk).
    watch: HashSet<PathBuf>,
}

/// A file's diff on one side, as read.
struct DiffRead {
    side: DiffSide,
    path: String,
    /// The path before a staged rename.
    orig_path: Option<String>,
    /// Whether the file's lines can be staged one by one ([`FileStatus::stages_by_line`]).
    by_line: bool,
    result: Result<FileDiff, String>,
}

pub struct GitPage {
    focus: FocusHandle,
    repos: Vec<RepoEntry>,
    repo: Option<RepoEntry>,
    /// `None` until read, and without a repository.
    status: Option<Result<RepoStatus, String>>,
    rows: Vec<StatusRow>,
    collapsed: HashSet<DiffSide>,
    /// The selected file: its group and path.
    selected: Option<(DiffSide, String)>,
    queue: VecDeque<Op>,
    running: bool,
    /// The repositories have been looked for at least once.
    searched: bool,
    shown: bool,
    watcher: FolderWatcher,
    /// The folders watched (none while hidden).
    watched: HashSet<PathBuf>,
    /// The folders to watch while the page shows, from the last status read.
    to_watch: HashSet<PathBuf>,
    scroll: UniformListScrollHandle,
    /// The diff on the right: the selected file's, or the one before while it is read.
    diff: Option<ShownDiff>,
    diff_scroll: UniformListScrollHandle,
    /// Where the keyboard goes in the diff (`focus` is the list's).
    diff_focus: FocusHandle,
    /// The selected file's diff has been read for a while: "Loading…" shows.
    loading: bool,
    /// Sets `loading` after a short delay; dropped when the diff arrives.
    loading_timer: Option<Task<()>>,
    /// The diff's selected lines and line cursor.
    lines: LineSelection,
    /// The mouse button went down on a changed line and is still held.
    dragging: bool,
    /// Lines of the shown diff were staged or unstaged: the index, in its changed lines, of the
    /// first one, where the cursor goes once the diff is read again.
    applied_at: Option<usize>,
    /// The commit message of the repository shown.
    message: Entity<TextareaState>,
    _message_events: Subscription,
    /// The commit messages typed for the other repositories, by top folder.
    drafts: HashMap<PathBuf, String>,
    /// A commit is queued or running.
    committing: bool,
}

impl WatchesFolders for GitPage {
    const OWNER: &'static str = "git page";

    fn ignores(event: &Event) -> bool {
        matches!(
            event.kind,
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
        )
    }

    fn on_folder_changes(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = &self.repo else {
            return;
        };
        if self.shown && paths.iter().any(|path| changes_status(path, &repo.git_dir)) {
            self.enqueue(Op::Refresh, window, cx);
        }
    }
}

impl GitPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (message, message_events) = commit_box::new_message(window, cx);
        Self {
            focus: cx.focus_handle(),
            repos: Vec::new(),
            repo: None,
            status: None,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            selected: None,
            queue: VecDeque::new(),
            running: false,
            searched: false,
            shown: false,
            watcher: FolderWatcher::new(window, cx),
            watched: HashSet::new(),
            to_watch: HashSet::new(),
            scroll: UniformListScrollHandle::new(),
            diff: None,
            diff_scroll: UniformListScrollHandle::new(),
            diff_focus: cx.focus_handle(),
            loading: false,
            loading_timer: None,
            lines: LineSelection::default(),
            dragging: false,
            applied_at: None,
            message,
            _message_events: message_events,
            drafts: HashMap::new(),
            committing: false,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The page shows: list the repositories of `folders` (the tabs' folders, the selected
    /// tab's first) and read the chosen one's status, then watch it. The one chosen is the
    /// repository `selected` (the selected tab's folder) is in, when given and in one.
    pub fn show(
        &mut self,
        folders: Vec<PathBuf>,
        selected: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.shown = true;
        self.enqueue(Op::FindRepos { folders, selected }, window, cx);
    }

    /// The page is hidden: stop watching (a running op still finishes).
    pub fn hide(&mut self) {
        self.shown = false;
        self.sync_watches();
    }

    /// The chosen repository's name, for the sidebar row.
    pub fn repo_name(&self) -> Option<&str> {
        self.repo.as_ref().map(|repo| repo.name.as_str())
    }

    fn enqueue(&mut self, op: Op, window: &mut Window, cx: &mut Context<Self>) {
        // A refresh or diff read already waiting covers this one (a diff read reads the
        // selection when it starts).
        let covered = match op {
            Op::Refresh => self
                .queue
                .iter()
                .any(|queued| matches!(queued, Op::Refresh)),
            Op::ReadDiff => self
                .queue
                .iter()
                .any(|queued| matches!(queued, Op::ReadDiff)),
            _ => false,
        };
        if covered {
            return;
        }
        self.queue.push_back(op);
        self.run_next(window, cx);
    }

    fn run_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let Some(op) = self.queue.pop_front() else {
            return;
        };
        self.running = true;
        let repo = self.repo.clone();
        let selection = self.selection();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_spawn(async move { perform(op, repo, selection) })
                .await;
            let _ = this.update_in(cx, |page, window, cx| page.finish(outcome, window, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish(&mut self, outcome: Outcome, window: &mut Window, cx: &mut Context<Self>) {
        let Outcome {
            repos,
            error,
            status,
            diff,
            commit,
        } = outcome;
        self.running = false;
        self.take_repos(repos);
        if let Some(read) = status {
            self.take_status(read, window, cx);
        }
        if let Some(message) = error {
            window.push_notification(Notification::error(message), cx);
        }
        if let Some(done) = commit {
            self.finish_commit(done, window, cx);
        }
        self.take_diff(diff);
        self.sync_diff(window, cx);
        cx.notify();
        self.run_next(window, cx);
    }

    /// Keep the repositories an op listed.
    fn take_repos(&mut self, repos: Option<Vec<RepoEntry>>) {
        if let Some(repos) = repos {
            self.repos = repos;
            self.searched = true;
        }
    }

    /// Show the repository and status an op read: its draft, rows, watches and placeholder.
    fn take_status(&mut self, read: StatusRead, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.repo.as_ref().map(|repo| repo.root.clone());
        let to = read.repo.as_ref().map(|repo| repo.root.clone());
        if from != to {
            self.swap_draft(from.as_deref(), to.as_deref(), window, cx);
        }
        // Found in the old rows while they still index the old status.
        let old_row = self.selected_row();
        self.repo = read.repo;
        self.status = read.status;
        self.to_watch = read.watch;
        self.rebuild_rows();
        self.reselect_near(old_row);
        self.sync_watches();
        self.sync_placeholder(window, cx);
    }

    /// Show the diff an op read, unless its file is no longer selected (a late read).
    fn take_diff(&mut self, diff: Option<DiffRead>) {
        if let Some(read) = diff.filter(|read| self.is_selected(read.side, &read.path)) {
            self.show_diff(read);
        }
    }

    /// The selection and its current status, for an op about to start.
    fn selection(&self) -> Option<Selection> {
        let (side, path) = self.selected.clone()?;
        let file = self
            .status()
            .and_then(|status| status.files_on(side).find(|file| file.path == path))
            .cloned();
        Some(Selection { side, path, file })
    }

    fn is_selected(&self, side: DiffSide, path: &str) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|(s, p)| *s == side && p == path)
    }

    /// Read the selected file's diff unless it shows; clear it without a selection.
    fn sync_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((side, path)) = self.selected.clone() else {
            self.diff = None;
            self.stop_loading();
            return;
        };
        if self.diff.as_ref().is_some_and(|diff| diff.is(side, &path)) {
            return;
        }
        if self.loading_timer.is_none() && !self.loading {
            self.loading_timer = Some(cx.spawn(async move |page, cx| {
                cx.background_executor()
                    .timer(diff_view::LOADING_DELAY)
                    .await;
                let _ = page.update(cx, |page, cx| {
                    page.loading = true;
                    cx.notify();
                });
            }));
        }
        self.enqueue(Op::ReadDiff, window, cx);
    }

    /// Show `read`. Read again for the same file, the scroll stays, and so does the line
    /// selection unless the lines changed; another file starts at the top with none.
    fn show_diff(&mut self, read: DiffRead) {
        let old = self
            .diff
            .take()
            .filter(|diff| diff.is(read.side, &read.path));
        let new = ShownDiff::new(read);
        match (old, self.applied_at.take()) {
            (None, _) => {
                diff_view::scroll_to_top(&self.diff_scroll);
                self.lines = LineSelection::default();
            }
            (Some(_), Some(at)) => self.lines.after_apply(new.changed_lines(), at),
            (Some(old), None) if old.same_lines(&new) => {}
            (Some(old), None) => match self.lines.cursor().and_then(|line| old.line_index(line)) {
                Some(at) => self.lines.after_apply(new.changed_lines(), at),
                None => self.lines = LineSelection::default(),
            },
        }
        self.diff = Some(new);
        self.stop_loading();
    }

    fn stop_loading(&mut self) {
        self.loading = false;
        self.loading_timer = None;
    }

    fn status(&self) -> Option<&RepoStatus> {
        self.status.as_ref()?.as_ref().ok()
    }

    /// Rebuild the rows from the status.
    fn rebuild_rows(&mut self) {
        self.rows = self
            .status()
            .map(|status| status.rows(&self.collapsed))
            .unwrap_or_default();
    }

    /// After the rows were rebuilt: a selected file that left its group gives the selection to
    /// the file row now at `old_row` (its row before), else to the last file row.
    fn reselect_near(&mut self, old_row: Option<usize>) {
        if self.selected_row().is_some() {
            return;
        }
        self.selected = old_row.and_then(|index| {
            let files = self.file_rows();
            let row = files
                .iter()
                .copied()
                .find(|&row| row >= index)
                .or_else(|| files.last().copied())?;
            self.row_file(row)
        });
    }

    /// The row of the selected file.
    fn selected_row(&self) -> Option<usize> {
        let (side, path) = self.selected.as_ref()?;
        let status = self.status()?;
        self.rows.iter().position(|row| match row {
            StatusRow::File { side: s, index } => s == side && status.files[*index].path == *path,
            StatusRow::Header { .. } => false,
        })
    }

    /// The indexes of the file rows.
    fn file_rows(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, StatusRow::File { .. }))
            .map(|(index, _)| index)
            .collect()
    }

    /// The group and path of the file at row `row`.
    fn row_file(&self, row: usize) -> Option<(DiffSide, String)> {
        let StatusRow::File { side, index } = *self.rows.get(row)? else {
            return None;
        };
        Some((side, self.status()?.files[index].path.clone()))
    }

    /// Watch what tells of a status change while the page shows, nothing while hidden.
    fn sync_watches(&mut self) {
        let wanted = if self.shown {
            self.to_watch.clone()
        } else {
            HashSet::new()
        };
        for folder in self.watched.difference(&wanted) {
            self.watcher.unwatch(folder);
        }
        for folder in wanted.difference(&self.watched) {
            if let Err(err) = self.watcher.watch(folder) {
                log::debug!("git page: not watching {}: {err}", folder.display());
            }
        }
        self.watched = wanted;
    }

    fn select(&mut self, file: (DiffSide, String), window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(file);
        self.sync_diff(window, cx);
        cx.notify();
    }

    fn toggle_group(&mut self, side: DiffSide, window: &mut Window, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&side) {
            self.collapsed.insert(side);
        }
        let old_row = self.selected_row();
        self.rebuild_rows();
        self.reselect_near(old_row);
        self.sync_diff(window, cx);
        cx.notify();
    }

    /// Stage a file of the Changes group, or unstage one of the Staged group.
    fn toggle_file(
        &mut self,
        side: DiffSide,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self
            .status()
            .and_then(|status| status.files.get(index))
            .cloned()
        else {
            return;
        };
        let change = match side {
            DiffSide::Unstaged => IndexChange::Stage(vec![file]),
            DiffSide::Staged => IndexChange::Unstage(vec![file]),
        };
        self.change_index(change, window, cx);
    }

    fn toggle_group_files(&mut self, side: DiffSide, window: &mut Window, cx: &mut Context<Self>) {
        let change = match side {
            DiffSide::Unstaged => IndexChange::StageAll,
            DiffSide::Staged => IndexChange::UnstageAll,
        };
        self.change_index(change, window, cx);
    }

    /// Queue `change` for the repository shown now.
    fn change_index(&mut self, change: IndexChange, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.target() {
            self.enqueue(Op::Change(target, change), window, cx);
        }
    }

    /// The repository shown now, as an op's target.
    fn target(&self) -> Option<Target> {
        let repo = self.repo.as_ref()?;
        Some(Target {
            root: repo.root.clone(),
            head: self.status().map(|status| status.head).unwrap_or_default(),
        })
    }

    /// In the list: Up / Down move the selection between files, Space stages or unstages the
    /// selected one. (Tab is [`FocusNext`].)
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "up" => self.step_selection(Direction::Up, window, cx),
            "down" => self.step_selection(Direction::Down, window, cx),
            "space" => {
                let Some(StatusRow::File { side, index }) =
                    self.selected_row().map(|row| self.rows[row])
                else {
                    return;
                };
                self.toggle_file(side, index, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(Direction::Down, window, cx);
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(Direction::Up, window, cx);
    }

    /// Move the keyboard to the next (Down) or previous (Up) of the commit message (when it
    /// shows), the list and the diff, around; from elsewhere to the list.
    fn cycle_focus(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let mut stops = Vec::with_capacity(3);
        if self.repo.is_some() {
            stops.push(self.message_focus(cx));
        }
        stops.push(self.focus.clone());
        stops.push(self.diff_focus.clone());
        let count = stops.len();
        let next = match (
            stops.iter().position(|stop| stop.is_focused(window)),
            direction,
        ) {
            (Some(at), Direction::Down) => (at + 1) % count,
            (Some(at), Direction::Up) => (at + count - 1) % count,
            (None, _) => count - 2,
        };
        window.focus(&stops[next], cx);
    }

    fn step_selection(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = self.file_rows();
        let next = match (self.selected_row(), direction) {
            (None, Direction::Down) => files.first(),
            (None, Direction::Up) => files.last(),
            (Some(row), Direction::Down) => files.iter().find(|&&file| file > row),
            (Some(row), Direction::Up) => files.iter().rev().find(|&&file| file < row),
        };
        let Some(&row) = next else {
            return;
        };
        if let Some(file) = self.row_file(row) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Top);
            self.select(file, window, cx);
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let repo_label: SharedString = match &self.repo {
            Some(repo) => repo.name.clone().into(),
            None => "No repository".into(),
        };
        let branch = self.status().map(|status| {
            status
                .branch
                .clone()
                .unwrap_or_else(|| "detached HEAD".to_string())
        });
        let page = cx.entity().downgrade();
        let (repos, current) = (self.repos.clone(), self.repo.clone());
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(HEADER_HEIGHT))
            .px_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("git-repo-picker")
                    .icon(Icon::empty().path(crate::assets::GIT_ICON))
                    .label(repo_label)
                    .ghost()
                    .small()
                    .disabled(self.repos.is_empty())
                    .tooltip("Repository")
                    .dropdown_menu(move |mut menu, _, _| {
                        for repo in &repos {
                            let checked = current.as_ref().is_some_and(|c| c.root == repo.root);
                            let (page, chosen) = (page.clone(), repo.clone());
                            let label = format!("{}  {}", repo.name, repo.root.display());
                            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                                move |_, window, cx| {
                                    let chosen = chosen.clone();
                                    let _ = page.update(cx, |page, cx| {
                                        page.enqueue(Op::Choose(chosen), window, cx);
                                    });
                                },
                            ));
                        }
                        menu
                    }),
            )
            .children(branch.map(|branch| {
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(Icon::empty().path(GIT_BRANCH_ICON).small())
                    .child(branch)
            }))
            .child(div().flex_1())
            .child(
                Button::new("git-refresh")
                    .icon(IconName::RefreshCw)
                    .ghost()
                    .small()
                    .loading(self.running)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|page, _, window, cx| {
                        page.enqueue(Op::Refresh, window, cx);
                    })),
            )
    }

    /// The list of changes, or why there is none.
    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let message = match (&self.status, &self.repo) {
            (_, None) if !self.searched => Some("Looking for repositories…".to_string()),
            (_, None) => Some("No open tab is in a git repository.".to_string()),
            (None, Some(_)) => Some("Reading the repository…".to_string()),
            (Some(Err(err)), Some(_)) => Some(format!("Couldn't read the repository: {err}")),
            (Some(Ok(_)), Some(_)) if self.rows.is_empty() => Some("No changes.".to_string()),
            (Some(Ok(_)), Some(_)) => None,
        };
        if let Some(message) = message {
            return div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(message)
                .into_any_element();
        }
        let truncated = self.status().is_some_and(|status| status.truncated);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                uniform_list(
                    "git-changes",
                    self.rows.len(),
                    cx.processor(|page, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| page.render_row(row, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1(),
            )
            .children(truncated.then(|| {
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Only the first {MAX_STATUS_FILES} changed files are listed."
                    ))
            }))
            .into_any_element()
    }

    fn render_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.rows[row] {
            StatusRow::Header { side, count } => self.render_group_header(side, count, cx),
            StatusRow::File { side, index } => self.render_file(row, side, index, cx),
        }
    }

    fn render_group_header(
        &self,
        side: DiffSide,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (title, icon, tooltip) = match side {
            DiffSide::Staged => ("Staged Changes", IconName::Minus, "Unstage all"),
            DiffSide::Unstaged => ("Changes", IconName::Plus, "Stage all"),
        };
        let chevron = if self.collapsed.contains(&side) {
            IconName::ChevronRight
        } else {
            IconName::ChevronDown
        };
        let group = SharedString::from(format!("git-group-{}", side_key(side)));
        div()
            .id(group.clone())
            .group(group.clone())
            .flex()
            .items_center()
            .gap_1()
            .h(px(ROW_HEIGHT))
            .px_2()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|style| style.bg(sidebar::hover_background(cx)))
            .on_click(cx.listener(move |page, _, window, cx| {
                page.toggle_group(side, window, cx);
            }))
            .child(
                Icon::new(chevron)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().child(title))
            .child(
                div()
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .child(
                        Button::new(SharedString::from(format!("git-all-{}", side_key(side))))
                            .icon(icon)
                            .ghost()
                            .xsmall()
                            .disabled(self.running)
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |page, _, window, cx| {
                                cx.stop_propagation();
                                page.toggle_group_files(side, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .px_1p5()
                    .rounded_full()
                    .bg(theme.secondary)
                    .text_color(theme.muted_foreground)
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_file(
        &self,
        row: usize,
        side: DiffSide,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(file) = self.status().and_then(|status| status.files.get(index)) else {
            return div().h(px(ROW_HEIGHT)).into_any_element();
        };
        let theme = cx.theme();
        let change = file.change(side).unwrap_or(Change::Modified);
        let (name, folder) = split_path(&file.path);
        let detail = match &file.orig_path {
            Some(orig) if side == DiffSide::Staged => format!("from {orig}"),
            _ => folder.to_string(),
        };
        let selected = self.selected_row() == Some(row);
        let (icon, tooltip) = match side {
            DiffSide::Staged => (IconName::Minus, "Unstage"),
            DiffSide::Unstaged => (IconName::Plus, "Stage"),
        };
        let group = SharedString::from(format!("git-file-{row}"));
        let clicked = (side, file.path.clone());
        let selected_background =
            crate::window_background::panel_background(theme.sidebar_accent, cx);
        let hovered = sidebar::hover_background(cx);
        div()
            .id(("git-file", row))
            .group(group.clone())
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW_HEIGHT))
            .pl_6()
            .pr_2()
            .text_sm()
            .when(selected, |row| row.bg(selected_background))
            .when(!selected, |row| row.hover(|style| style.bg(hovered)))
            .on_click(cx.listener(move |page, _, window, cx| {
                window.focus(&page.focus, cx);
                page.select(clicked.clone(), window, cx);
            }))
            .child(
                div()
                    .flex_none()
                    .max_w(px(FILE_NAME_MAX_WIDTH))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(change == Change::Deleted, |name| name.line_through())
                    .child(name.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(detail),
            )
            .child(
                div()
                    .invisible()
                    .group_hover(group, |style| style.visible())
                    .child(
                        Button::new(("git-toggle-file", row))
                            .icon(icon)
                            .ghost()
                            .xsmall()
                            .disabled(self.running)
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |page, _, window, cx| {
                                cx.stop_propagation();
                                page.toggle_file(side, index, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(CHANGE_LETTER_WIDTH))
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(change_color(change, cx))
                    .child(change.letter().to_string()),
            )
            .into_any_element()
    }
}

impl Render for GitPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border) = (theme.background, theme.border);
        div()
            .id("git-page")
            .debug_selector(|| "git-page".into())
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .on_key_down(cx.listener(Self::key_down))
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .w(px(LIST_WIDTH))
                            .h_full()
                            .border_r_1()
                            .border_color(border)
                            .when(self.repo.is_some(), |column| {
                                column.child(self.render_commit_box(cx))
                            })
                            .child(self.render_list(cx)),
                    )
                    .child(self.render_diff_pane(window, cx)),
            )
    }
}

/// What [`run_op`] did.
struct OpDone {
    /// The new list of repositories, when the op listed them.
    repos: Option<Vec<RepoEntry>>,
    /// The repository to read the status of afterwards.
    repo: Option<RepoEntry>,
    error: Option<String>,
    commit: Option<CommitDone>,
}

/// Run `op`, then read the status of `repo` (or of the one `op` picks) and the diff of
/// `selection`; for [`Op::ReadDiff`], the diff only. Off the UI thread.
fn perform(op: Op, repo: Option<RepoEntry>, selection: Option<Selection>) -> Outcome {
    if matches!(op, Op::ReadDiff) {
        return Outcome::diff_only(read_selection_diff(repo, selection));
    }
    let done = run_op(op, repo);
    let read = read_state(done.repo);
    let diff = selection.and_then(|selection| read_listed_diff(&read, selection));
    Outcome {
        repos: done.repos,
        error: done.error,
        status: Some(read),
        diff,
        commit: done.commit,
    }
}

/// Run what `op` asks of git, `repo` being the repository shown when it started.
fn run_op(op: Op, repo: Option<RepoEntry>) -> OpDone {
    let mut done = OpDone {
        repos: None,
        repo,
        error: None,
        commit: None,
    };
    match op {
        Op::FindRepos { folders, selected } => done.find_repos(&folders, selected.as_deref()),
        Op::Choose(chosen) => done.repo = Some(chosen),
        Op::Refresh | Op::ReadDiff => {}
        Op::Change(target, change) => {
            done.error = change_index(&target, change)
                .err()
                .map(|err| err.to_string());
        }
        Op::Commit(target, message) => done.commit(target, &message),
    }
    done
}

impl OpDone {
    /// List the repositories of `folders` and choose one (`git::choose_repo`).
    fn find_repos(&mut self, folders: &[PathBuf], selected: Option<&Path>) {
        let found = git::repos_of(folders);
        let selected = selected.and_then(git::locate);
        self.repo = git::choose_repo(&found, selected, self.repo.take());
        self.repos = Some(found);
    }

    /// Commit what is staged in `target` with `message`.
    fn commit(&mut self, target: Target, message: &str) {
        let committed = git::commit(&target.root, message);
        self.error = committed.as_ref().err().map(git::commit_failure);
        self.commit = Some(CommitDone {
            root: target.root,
            summary: committed.ok(),
        });
    }
}

impl Outcome {
    /// What an [`Op::ReadDiff`] gives: the diff only.
    fn diff_only(diff: Option<DiffRead>) -> Self {
        Self {
            repos: None,
            error: None,
            status: None,
            diff,
            commit: None,
        }
    }
}

/// The status of `repo` and the folders to watch for it.
fn read_state(repo: Option<RepoEntry>) -> StatusRead {
    let status = repo
        .as_ref()
        .map(|repo| git::read_status(&repo.root).map_err(|err| err.to_string()));
    let watch = repo.as_ref().map_or_else(HashSet::new, |repo| {
        watched_folders(
            repo,
            status.as_ref().and_then(|status| status.as_ref().ok()),
        )
    });
    StatusRead {
        repo,
        status,
        watch,
    }
}

/// The diff of the selected file as `read` lists it, if it still has a change on its side.
fn read_listed_diff(read: &StatusRead, selection: Selection) -> Option<DiffRead> {
    let root = &read.repo.as_ref()?.root;
    let file = read
        .status
        .as_ref()?
        .as_ref()
        .ok()?
        .files_on(selection.side)
        .find(|file| file.path == selection.path)?;
    Some(read_file_diff(root, file, selection.side))
}

/// The diff of the selected file as listed when the read was asked.
fn read_selection_diff(repo: Option<RepoEntry>, selection: Option<Selection>) -> Option<DiffRead> {
    let (repo, Selection { side, path, file }) = repo.zip(selection)?;
    Some(match file {
        Some(file) => read_file_diff(&repo.root, &file, side),
        None => DiffRead {
            side,
            path,
            orig_path: None,
            by_line: false,
            result: Err(NO_LONGER_CHANGED.to_string()),
        },
    })
}

fn read_file_diff(root: &Path, file: &FileStatus, side: DiffSide) -> DiffRead {
    DiffRead {
        side,
        path: file.path.clone(),
        orig_path: file.orig_path.clone().filter(|_| side == DiffSide::Staged),
        by_line: file.stages_by_line(side),
        result: git::read_diff(root, file, side).map_err(|err| err.to_string()),
    }
}

fn change_index(target: &Target, change: IndexChange) -> Result<(), git::GitError> {
    let root = &target.root;
    match change {
        IndexChange::Stage(files) => git::stage(root, &refs(&files)),
        IndexChange::Unstage(files) => git::unstage(root, &refs(&files), target.head),
        IndexChange::StageAll => git::stage_all(root),
        IndexChange::UnstageAll => git::unstage_all(root, target.head),
        IndexChange::Apply { side, patch } => git::apply_patch(root, patch, side),
    }
}

fn refs(files: &[FileStatus]) -> Vec<&FileStatus> {
    files.iter().collect()
}

/// The folders to watch: the git folder (`index`, `HEAD`), the top folder and those of the
/// changed files, which an edit of one of them changes. Reads the disk (once per folder): call
/// it off the UI thread.
fn watched_folders(repo: &RepoEntry, status: Option<&RepoStatus>) -> HashSet<PathBuf> {
    let mut folders = HashSet::from([repo.git_dir.clone(), repo.root.clone()]);
    let mut checked = HashSet::new();
    let changed = status
        .into_iter()
        .flat_map(|status| &status.files)
        .filter_map(|file| repo.root.join(&file.path).parent().map(Path::to_path_buf));
    for folder in changed {
        if folders.len() >= MAX_WATCHED_FOLDERS + 2 {
            break;
        }
        if !folders.contains(&folder) && checked.insert(folder.clone()) && folder.is_dir() {
            folders.insert(folder);
        }
    }
    folders
}

/// Whether a change at `path` may change the status: anything in the work tree, and in the
/// git folder only `index` and `HEAD` (not the lock files and logs git writes as it works).
fn changes_status(path: &Path, git_dir: &Path) -> bool {
    if !path.starts_with(git_dir) {
        return true;
    }
    path.parent() == Some(git_dir)
        && path
            .file_name()
            .is_some_and(|name| GIT_DIR_FILES.iter().any(|file| name == *file))
}

/// A path's file name and its folder (empty at the top).
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((folder, name)) => (name, folder),
        None => (path, ""),
    }
}

fn side_key(side: DiffSide) -> &'static str {
    match side {
        DiffSide::Staged => "staged",
        DiffSide::Unstaged => "unstaged",
    }
}

fn change_color(change: Change, cx: &App) -> Hsla {
    let theme = cx.theme();
    match change {
        Change::Added | Change::Untracked => theme.success,
        Change::Deleted | Change::Conflicted => theme.danger,
        Change::Modified | Change::TypeChanged | Change::Renamed | Change::Copied => theme.warning,
    }
}

mod commit_box;
mod diff_view;
mod line_staging;
#[cfg(test)]
mod tests;
