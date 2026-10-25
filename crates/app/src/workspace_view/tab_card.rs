//! The hover card next to a tab row: the tab's details, the `+N` lines added from `git diff`
//! and the ticking lap or running time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::component::WindowExt;
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::{
    Anchor, AnyElement, App, AppContext, Context, IntoElement, Styled, Task, Window, div,
};
use workspace_model::{TabCardField, display_path, lines_added, until_next_second};

use super::WorkspaceView;
use crate::launcher;
use crate::settings_file;
use crate::sidebar::{self, TabCard};

/// How long a hover card's `+N` is reused before `git diff` runs again.
const DIFF_STAT_TTL: Duration = Duration::from_secs(5);

/// The `+N` shown in a tab's hover card, for the folder it was computed in.
struct DiffStat {
    folder: PathBuf,
    /// When `git diff` last answered for `folder`.
    checked: Option<Instant>,
    /// The run whose answer is awaited; an answer from an older run is stale.
    generation: u64,
    running: bool,
    /// `None` until `git diff` answers, or when it cannot.
    added: Option<u64>,
}

impl DiffStat {
    fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            checked: None,
            generation: 0,
            running: false,
            added: None,
        }
    }

    /// Whether `git diff` should run for `folder`: not while a run for it is going, nor when one
    /// answered within [`DIFF_STAT_TTL`].
    fn needs_run(&self, folder: &Path, now: Instant) -> bool {
        if self.folder != folder {
            return true;
        }
        let fresh = self
            .checked
            .is_some_and(|checked| now.duration_since(checked) < DIFF_STAT_TTL);
        !self.running && !fresh
    }

    /// Start a run for `folder` and return its generation. The last answer stays shown while it
    /// runs, unless the folder changed.
    fn start(&mut self, folder: &Path) -> u64 {
        if self.folder != folder {
            *self = Self {
                generation: self.generation,
                ..Self::new(folder.to_path_buf())
            };
        }
        self.generation += 1;
        self.running = true;
        self.generation
    }

    /// Take the answer of run `generation`. Returns false, ignoring it, when a newer run
    /// started since (the folder changed while it ran).
    fn finish(&mut self, generation: u64, added: Option<u64>, now: Instant) -> bool {
        if generation != self.generation {
            return false;
        }
        self.running = false;
        self.checked = Some(now);
        self.added = added;
        true
    }
}

/// What the hover cards keep between renders.
#[derive(Default)]
pub(super) struct TabCards {
    /// Lines added per tab, fetched when its hover card opens.
    diff_stats: HashMap<u64, DiffStat>,
    shown: Option<u64>,
    /// The tab whose hover card shows, and the task repainting its lap time each second.
    clock: Option<(u64, Task<()>)>,
    /// No card shows, and the clock stops, while a tab is dragged or something the user works
    /// in (a name editor, a menu, a dialog) is open. Set at each render.
    pub(super) held: bool,
    /// The tab whose card was open when a hold ended: it stays hidden until the pointer
    /// leaves its row, so a card answers a hover, not the end of a menu, dialog or drag.
    suppressed: Option<u64>,
    /// The same two for the group cards, by sidebar slot.
    pub(super) group_shown: Option<usize>,
    pub(super) group_suppressed: Option<usize>,
}

impl TabCards {
    /// Forget tab `id`'s `+N`: the tab is gone, or runs another session.
    pub fn forget(&mut self, id: u64) {
        self.diff_stats.remove(&id);
        if self.shown == Some(id) {
            self.shown = None;
            self.clock = None;
        }
    }

    /// Start a `git diff` run for tab `id` in `folder`, unless one is going or answered recently.
    /// Returns the run's generation.
    fn start_diff_stat(&mut self, id: u64, folder: &Path, now: Instant) -> Option<u64> {
        let stat = self
            .diff_stats
            .entry(id)
            .or_insert_with(|| DiffStat::new(folder.to_path_buf()));
        stat.needs_run(folder, now).then(|| stat.start(folder))
    }

    /// Take the answer of tab `id`'s run `generation`. Returns whether it is kept (no newer run
    /// started since).
    fn finish_diff_stat(
        &mut self,
        id: u64,
        generation: u64,
        added: Option<u64>,
        now: Instant,
    ) -> bool {
        self.diff_stats
            .get_mut(&id)
            .is_some_and(|stat| stat.finish(generation, added, now))
    }

    /// Lines added in `folder` for tab `id`, once `git diff` answered there.
    fn lines_added(&self, id: u64, folder: &Path) -> Option<u64> {
        self.diff_stats
            .get(&id)
            .filter(|stat| stat.folder == folder)
            .and_then(|stat| stat.added)
    }
}

impl WorkspaceView {
    /// Hold the hover cards back (see [`TabCards::held`]), checked at each render. A held card
    /// is not shown and its clock stops; one open when the hold ends stays hidden until its
    /// row is left.
    pub(super) fn hold_hover_cards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held = self.holds_hover_cards(window, cx);
        if held == self.cards.held {
            return;
        }
        self.cards.held = held;
        if held {
            self.cards.clock = None;
            return;
        }
        if self.cards.shown.is_some() {
            self.cards.suppressed = self.cards.shown;
        }
        if self.cards.group_shown.is_some() {
            self.cards.group_suppressed = self.cards.group_shown;
        }
    }

    fn holds_hover_cards(&self, window: &mut Window, cx: &mut App) -> bool {
        cx.has_active_drag()
            || self.renaming.is_some()
            || self.menu.is_open()
            || self.explorer.read(cx).holds_popups()
            || window.has_active_dialog(cx)
    }

    /// Tab `id`'s hover card opened (its row was hovered for a moment).
    fn card_opened(&mut self, id: u64, cx: &mut Context<Self>) {
        self.cards.shown = Some(id);
        if self.cards.held || self.cards.suppressed == Some(id) {
            return;
        }
        self.refresh_diff_stat(id, cx);
        self.start_card_clock(id, cx);
    }

    /// Tab `id`'s hover card closed: the pointer left its row.
    fn card_closed(&mut self, id: u64, cx: &mut Context<Self>) {
        self.stop_card_clock(id);
        if self.cards.shown == Some(id) {
            self.cards.shown = None;
        }
        if self.cards.suppressed == Some(id) {
            self.cards.suppressed = None;
            // The card's `appearance` was fixed at the last render.
            cx.notify();
        }
    }

    /// Group slot `slot`'s hover card opened or closed.
    pub(super) fn group_card_changed(&mut self, slot: usize, open: bool, cx: &mut Context<Self>) {
        if open {
            self.cards.group_shown = Some(slot);
            return;
        }
        if self.cards.group_shown == Some(slot) {
            self.cards.group_shown = None;
        }
        if self.cards.group_suppressed == Some(slot) {
            self.cards.group_suppressed = None;
            cx.notify();
        }
    }

    /// The tab's details in a card next to the row, after hovering it for a moment. Opening it
    /// runs `git diff` for the `+N` (cached for a few seconds) and, while it shows, ticks the
    /// lap time of a working agent. The card is built only while it shows, and not while held
    /// back: it stays wrapped then, since unwrapping the row would reset the row's open menu.
    pub(super) fn with_hover_card(
        &self,
        row: AnyElement,
        id: u64,
        cx: &Context<Self>,
    ) -> AnyElement {
        if !settings_file::settings(cx).tab_card.active() {
            return row;
        }
        let held = self.cards.held || self.cards.suppressed == Some(id);
        let workspace = cx.entity().downgrade();
        let source = workspace.clone();
        HoverCard::new(("tab-card", id as usize))
            .anchor(Anchor::LeftCenter)
            .ml_2()
            .appearance(!held)
            .trigger(row)
            .content(move |_, _, cx| {
                let card = source
                    .upgrade()
                    .filter(|_| !held)
                    .and_then(|workspace| workspace.read(cx).tab_card(id, cx));
                match card {
                    Some(card) => sidebar::tab_card(card, cx).into_any_element(),
                    None => div().into_any_element(),
                }
            })
            .on_open_change(move |open, _, cx| {
                let _ = workspace.update(cx, |workspace, cx| match *open {
                    true => workspace.card_opened(id, cx),
                    false => workspace.card_closed(id, cx),
                });
            })
            .into_any_element()
    }

    fn tab_card(&self, id: u64, cx: &App) -> Option<TabCard> {
        let fields = settings_file::settings(cx).tab_card;
        if !fields.active() {
            return None;
        }
        let index = self.tab_index(id)?;
        let tab = &self.tabs[index];
        let view = tab.view.read(cx);
        let location = view.location();
        let now = Instant::now();
        Some(TabCard {
            index,
            name: Self::tab_name(tab, cx).into(),
            status: fields.keep(TabCardField::Status, || view.tab_status()),
            mode: fields
                .keep(TabCardField::SessionKind, || view.agent_mode())
                .flatten(),
            working_for: fields
                .keep(TabCardField::Status, || view.working_for(now))
                .flatten(),
            running_for: fields
                .keep(TabCardField::Status, || view.running_for(now))
                .flatten(),
            running_label: fields
                .keep(TabCardField::Status, || {
                    view.running_label().map(|label| label.to_string().into())
                })
                .flatten(),
            folder: fields
                .keep(TabCardField::Location, || {
                    location
                        .map(|location| display_path(&location.folder, self.home.as_deref()).into())
                })
                .flatten(),
            branch: fields
                .keep(TabCardField::Location, || {
                    location
                        .and_then(|location| location.head.as_ref())
                        .map(|head| head.name().to_string().into())
                })
                .flatten(),
            title: fields
                .keep(TabCardField::ProgramTitle, || {
                    tab.meta
                        .custom_name
                        .as_ref()
                        .filter(|name| name.as_str() != view.name())
                        .map(|_| view.name().to_string().into())
                })
                .flatten(),
            kind: fields.keep(TabCardField::SessionKind, || view.kind()),
            kind_label: fields.keep(TabCardField::SessionKind, || {
                launcher::default_name(&tab.spec).into()
            }),
            lines_added: fields
                .keep(TabCardField::Changes, || {
                    location.and_then(|location| self.cards.lines_added(id, &location.folder))
                })
                .flatten(),
        })
    }

    pub(super) fn update_card_settings(&mut self, cx: &mut Context<Self>) {
        let fields = settings_file::settings(cx).tab_card;
        if !fields.keeps(TabCardField::Status) {
            self.cards.clock = None;
        }
        if !fields.active() {
            self.cards.shown = None;
            return;
        }
        if let Some(id) = self.cards.shown {
            if self.cards.clock.is_none() {
                self.start_card_clock(id, cx);
            }
            self.refresh_diff_stat(id, cx);
        }
    }

    /// Stop the clock when this tab's card closes.
    fn stop_card_clock(&mut self, id: u64) {
        if self
            .cards
            .clock
            .as_ref()
            .is_some_and(|(shown, _)| *shown == id)
        {
            self.cards.clock = None;
        }
    }

    /// Repaint the hover card while it shows. The window repaints on each whole second while
    /// the agent is working or a command runs; otherwise the task checks once a second whether
    /// one started.
    fn start_card_clock(&mut self, id: u64, cx: &mut Context<Self>) {
        if !settings_file::settings(cx)
            .tab_card
            .keeps(TabCardField::Status)
        {
            self.stop_card_clock(id);
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                let Ok(ticking) = this.read_with(cx, |workspace, cx| {
                    let index = workspace.tab_index(id)?;
                    let view = workspace.tabs[index].view.read(cx);
                    let now = Instant::now();
                    view.working_for(now).or_else(|| view.running_for(now))
                }) else {
                    return;
                };
                let delay = ticking.map_or(Duration::from_secs(1), until_next_second);
                cx.background_executor().timer(delay).await;
                if ticking.is_some() && this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        });
        self.cards.clock = Some((id, task));
    }

    /// Run `git diff` for the tab's hover card unless a recent answer for its folder is cached.
    fn refresh_diff_stat(&mut self, id: u64, cx: &mut Context<Self>) {
        if !settings_file::settings(cx)
            .tab_card
            .keeps(TabCardField::Changes)
        {
            return;
        }
        let Some(location) = self
            .tab_index(id)
            .and_then(|index| self.tabs[index].view.read(cx).location())
            .filter(|location| location.head.is_some())
        else {
            return;
        };
        let folder = location.folder.clone();
        let Some(generation) = self.cards.start_diff_stat(id, &folder, Instant::now()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let added = cx
                .background_spawn(async move { lines_added(&folder) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace
                    .cards
                    .finish_diff_stat(id, generation, added, Instant::now())
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
