//! The Git page's branch picker. The header's branch label opens it (or B from the list or
//! the diff): a filter over the local branches, then the remote-tracking ones, read when it
//! opens (never on refresh). Choosing a branch switches to it once the working tree is clean
//! and the workspace has asked about busy tabs and unsaved files in it
//! ([`GitPageEvent::SwitchAsked`]); after the switch every tab re-reads its branch
//! ([`GitPageEvent::Switched`]).

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{ActiveTheme, Icon, IconName, IndexPath, Sizable, WindowExt};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, Focusable, InteractiveElement, IntoElement,
    ParentElement, SharedString, Styled, Task, WeakEntity, Window, actions, div, px,
};
use workspace_model::git::{self, BranchChoice, Branches, SwitchPlan};

use super::{GitPage, Op};
use crate::assets::GIT_BRANCH_ICON;

/// The picker's key context, around its list.
const PICKER_CONTEXT: &str = "GitBranchPicker";
/// The picker's width.
const PICKER_WIDTH: f32 = 340.;
/// The most the list grows before it scrolls.
const PICKER_LIST_HEIGHT: f32 = 320.;
/// The sections, in order.
const SECTIONS: [&str; 2] = ["Local", "Remote"];

actions!(
    git_page,
    [
        /// Space in the picker's filter: branch names hold no space, and the popover would
        /// take it as choosing the selected branch.
        IgnoreSpace
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([gpui_kit::KeyBinding::new(
        "space",
        IgnoreSpace,
        Some(PICKER_CONTEXT),
    )]);
}

/// What the page tells the workspace about switching branches.
pub enum GitPageEvent {
    /// A branch was chosen in a clean working tree: ask about busy tabs and unsaved files in
    /// it, then [`GitPage::start_switch`].
    SwitchAsked(SwitchRequest),
    /// A switch ran (also when git failed: a failing hook still switched): the tabs re-read
    /// their branch.
    Switched,
}

/// A switch waiting for the workspace's go.
#[derive(Clone, Debug)]
pub struct SwitchRequest {
    /// The working tree's top folder.
    pub root: PathBuf,
    /// The branch checked out after it.
    pub branch: String,
    /// What `git` runs ([`SwitchPlan::Run`]).
    args: Vec<String>,
}

/// A switch the op queue ran or refused.
pub(super) struct SwitchDone {
    /// Git's message on success.
    summary: Option<String>,
    /// `git switch` ran (it may have changed files even when it failed).
    ran: bool,
}

/// The picker's list: the branches read, filtered by the query.
pub(super) struct BranchDelegate {
    page: WeakEntity<GitPage>,
    /// `None` while read.
    branches: Option<Result<Branches, String>>,
    /// The filter, lowercased.
    query: String,
    /// The indexes shown, per section (local, remote).
    shown: [Vec<usize>; 2],
    selected: Option<IndexPath>,
}

impl BranchDelegate {
    fn new(page: WeakEntity<GitPage>) -> Self {
        Self {
            page,
            branches: None,
            query: String::new(),
            shown: [Vec::new(), Vec::new()],
            selected: None,
        }
    }

    fn set_branches(&mut self, branches: Option<Result<Branches, String>>) {
        self.branches = branches;
        self.filter();
    }

    /// Show the branches whose name holds the query, ignoring case.
    fn filter(&mut self) {
        let Some(Ok(branches)) = &self.branches else {
            self.shown = [Vec::new(), Vec::new()];
            return;
        };
        let local = branches.local.iter().map(|branch| branch.name.as_str());
        let remote = branches.remote.iter().map(|branch| branch.name.as_str());
        self.shown = [matching(local, &self.query), matching(remote, &self.query)];
    }

    /// The first row shown, in the first section with one.
    fn first_row(&self) -> Option<IndexPath> {
        let section = self.shown.iter().position(|rows| !rows.is_empty())?;
        Some(IndexPath::new(0).section(section))
    }

    fn branches(&self) -> Option<&Branches> {
        self.branches.as_ref()?.as_ref().ok()
    }

    /// The branch at `ix`, as chosen.
    fn choice(&self, ix: IndexPath) -> Option<BranchChoice> {
        let branches = self.branches()?;
        let index = *self.shown.get(ix.section)?.get(ix.row)?;
        Some(match ix.section {
            0 => BranchChoice::Local(branches.local.get(index)?.name.clone()),
            _ => BranchChoice::Remote(branches.remote.get(index)?.clone()),
        })
    }

    /// The row at `ix`: its name, whether it is checked out here, and a note.
    fn row(&self, ix: IndexPath) -> Option<(String, bool, Option<&'static str>)> {
        let branches = self.branches()?;
        let index = *self.shown.get(ix.section)?.get(ix.row)?;
        if ix.section == 0 {
            let branch = branches.local.get(index)?;
            let note = branch.elsewhere.then_some("in another worktree");
            return Some((branch.name.clone(), branch.current, note));
        }
        Some((branches.remote.get(index)?.name.clone(), false, None))
    }
}

/// The indexes of `names` holding `query` (lowercase), ignoring case.
fn matching<'a>(names: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    names
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(query))
        .map(|(index, _)| index)
        .collect()
}

impl ListDelegate for BranchDelegate {
    type Item = ListItem;

    /// Filter, then select the first row shown (the list selects the first row of the first
    /// section, which may be empty).
    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.to_lowercase();
        self.filter();
        cx.spawn_in(window, async move |list, cx| {
            let _ = list.update_in(cx, select_first);
        })
    }

    fn sections_count(&self, _: &App) -> usize {
        SECTIONS.len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.shown.get(section).map_or(0, Vec::len)
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let (name, current, note) = self.row(ix)?;
        let muted = cx.theme().muted_foreground;
        let id = SharedString::from(format!("git-branch-{}-{}", ix.section, ix.row));
        Some(
            ListItem::new(id)
                .check_icon(IconName::Check)
                .confirmed(current)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().min_w_0().truncate().child(name))
                        .children(note.map(|note| div().text_xs().text_color(muted).child(note))),
                ),
        )
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let title = SECTIONS.get(section)?;
        Some(
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(*title),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = match &self.branches {
            Some(Err(err)) => format!("Couldn't read the branches: {err}"),
            _ => "No branch matches.".to_string(),
        };
        div()
            .p_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }

    fn loading(&self, _: &App) -> bool {
        self.branches.is_none()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    /// Enter or a click: hand the branch and what switching to it does to the page (the list
    /// can't be read from the page while this runs).
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(choice) = self.selected.and_then(|ix| self.choice(ix)) else {
            return;
        };
        let Some(branches) = self.branches() else {
            return;
        };
        let plan = git::switch_plan(branches, &choice);
        let branch = choice.local_name().to_string();
        let _ = self.page.update(cx, |page, cx| {
            page.pick_branch(branch, plan, window, cx);
        });
    }
}

/// Select the first row `list` shows.
fn select_first(
    list: &mut ListState<BranchDelegate>,
    window: &mut Window,
    cx: &mut Context<ListState<BranchDelegate>>,
) {
    let first = list.delegate().first_row();
    list.set_selected_index(first, window, cx);
    cx.notify();
}

/// The picker's list, searchable, made with the page.
pub(super) fn new_branch_list(
    window: &mut Window,
    cx: &mut Context<GitPage>,
) -> Entity<ListState<BranchDelegate>> {
    let delegate = BranchDelegate::new(cx.weak_entity());
    cx.new(|cx| ListState::new(delegate, window, cx).searchable(true))
}

impl GitPage {
    /// Open the picker and read the branches (the label shows only with a status).
    pub(super) fn open_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status().is_none() {
            return;
        }
        self.picker_open = true;
        self.picker_return = window.focused(cx);
        self.branch_list.update(cx, |list, cx| {
            list.delegate_mut().set_branches(None);
            list.set_query("", window, cx);
        });
        self.enqueue(Op::ListBranches, window, cx);
        cx.notify();
    }

    /// The popover closed (Escape, a click outside): it gives the keyboard back itself.
    pub(super) fn close_branch_picker(&mut self, cx: &mut Context<Self>) {
        self.picker_open = false;
        self.picker_return = None;
        cx.notify();
    }

    /// Close the picker if it is open, the keyboard back where it was before it opened (the
    /// command palette opening over it).
    pub(crate) fn dismiss_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker_open {
            self.close_branch_picker_with_focus(window, cx);
        }
    }

    /// The picker closes on a choice: the keyboard goes back at once, not on the popover's
    /// closing redraw, which a question opened meanwhile could overtake (it would record the
    /// closed filter as the focus to restore).
    pub(super) fn close_branch_picker_with_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = self
            .picker_return
            .take()
            .unwrap_or_else(|| self.focus.clone());
        window.focus(&back, cx);
        self.close_branch_picker(cx);
    }

    /// Show the branches an op read, the first one selected.
    pub(super) fn take_branches(
        &mut self,
        branches: Result<Branches, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.branch_list.update(cx, |list, cx| {
            list.delegate_mut().set_branches(Some(branches));
            select_first(list, window, cx);
        });
    }

    /// A branch was chosen: the picker closes; the current branch does nothing; a dirty working
    /// tree (as shown) or a refused plan shows why, where op errors show; otherwise the
    /// workspace is asked to go on.
    fn pick_branch(
        &mut self,
        branch: String,
        plan: SwitchPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_branch_picker_with_focus(window, cx);
        if matches!(plan, SwitchPlan::Nothing) {
            return;
        }
        if let Some(reason) = self.switch_blocker() {
            window.push_notification(Notification::error(reason), cx);
            return;
        }
        match plan {
            SwitchPlan::Nothing => {}
            SwitchPlan::Refuse(reason) => {
                window.push_notification(Notification::error(reason), cx);
            }
            SwitchPlan::Run(args) => self.ask_switch(branch, args, cx),
        }
    }

    fn ask_switch(&mut self, branch: String, args: Vec<String>, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else {
            return;
        };
        let root = repo.root.clone();
        cx.emit(GitPageEvent::SwitchAsked(SwitchRequest {
            root,
            branch,
            args,
        }));
    }

    /// Why the shown working tree can't switch branches, if it can't.
    fn switch_blocker(&self) -> Option<String> {
        self.status().and_then(git::switch_blocker)
    }

    /// Run a switch the workspace let go on (after its question, if it asked one).
    pub fn start_switch(
        &mut self,
        request: SwitchRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enqueue(Op::Switch(request), window, cx);
    }

    /// A switch ended: tell the workspace when it ran, and show git's message on success.
    pub(super) fn finish_switch(
        &mut self,
        done: SwitchDone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if done.ran {
            cx.emit(GitPageEvent::Switched);
        }
        if let Some(summary) = done.summary.filter(|summary| !summary.is_empty()) {
            window.push_notification(Notification::info(summary), cx);
        }
    }

    /// The header's branch label, opening the picker.
    pub(super) fn render_branch_picker(
        &self,
        branch: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page = cx.entity().downgrade();
        let list = self.branch_list.clone();
        let blocker = self.switch_blocker().map(SharedString::from);
        Popover::new("git-branch-picker")
            .open(self.picker_open)
            .on_open_change(move |open, window, cx| {
                let _ = page.update(cx, |page, cx| match *open {
                    true => page.open_branch_picker(window, cx),
                    false => page.close_branch_picker(cx),
                });
            })
            .track_focus(&self.branch_list.focus_handle(cx))
            .trigger(
                Button::new("git-branch")
                    .icon(Icon::empty().path(GIT_BRANCH_ICON))
                    .label(branch)
                    .ghost()
                    .small()
                    .tooltip("Switch branch (B)"),
            )
            .content(move |_, _, cx| picker_content(&list, blocker.clone(), cx))
            .into_any_element()
    }
}

/// The popover's content: why a switch is blocked (when it is) above the filter and list.
fn picker_content(
    list: &Entity<ListState<BranchDelegate>>,
    blocker: Option<SharedString>,
    cx: &App,
) -> impl IntoElement + use<> {
    div()
        .id("git-branch-picker-content")
        .key_context(PICKER_CONTEXT)
        .on_action(|_: &IgnoreSpace, _, _| {})
        .flex()
        .flex_col()
        .gap_2()
        .w(px(PICKER_WIDTH))
        .children(
            blocker.map(|reason| div().text_xs().text_color(cx.theme().warning).child(reason)),
        )
        .child(List::new(list).max_h(px(PICKER_LIST_HEIGHT)))
}

/// Switch `request`'s working tree when it is still clean (the status shown may be stale).
/// Off the UI thread.
pub(super) fn run_switch(request: SwitchRequest) -> (SwitchDone, Option<String>) {
    let SwitchRequest { root, args, .. } = request;
    let blocker = match git::read_status(&root) {
        Ok(status) => git::switch_blocker(&status),
        Err(err) => Some(err.to_string()),
    };
    if let Some(reason) = blocker {
        let done = SwitchDone {
            summary: None,
            ran: false,
        };
        return (done, Some(reason));
    }
    let switched = git::switch_branch(&root, &args);
    let error = switched.as_ref().err().map(ToString::to_string);
    let done = SwitchDone {
        summary: switched.ok(),
        ran: true,
    };
    (done, error)
}

#[cfg(test)]
mod tests;
