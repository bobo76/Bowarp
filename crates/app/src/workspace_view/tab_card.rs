//! The hover card next to a tab row: the tab's details, the `+N` lines added from `git diff`
//! and the ticking lap or running time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::{Anchor, AnyElement, App, AppContext, Context, IntoElement, Styled, Task, div};
use workspace_model::{display_path, lines_added, until_next_second};

use super::WorkspaceView;
use crate::launcher;
use crate::sidebar::{self, TabCard};

/// How long a hover card's `+N` is reused before `git diff` runs again.
const DIFF_STAT_TTL: Duration = Duration::from_secs(5);

/// The `+N` shown in a tab's hover card, for the folder it was computed in.
struct DiffStat {
    dir: PathBuf,
    /// When `git diff` last answered for `dir`.
    checked: Option<Instant>,
    /// The run whose answer is awaited; an answer from an older run is stale.
    generation: u64,
    running: bool,
    /// `None` until `git diff` answers, or when it cannot.
    added: Option<u64>,
}

impl DiffStat {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            checked: None,
            generation: 0,
            running: false,
            added: None,
        }
    }

    /// Whether `git diff` should run for `dir`: not while a run for it is going, nor when one
    /// answered within [`DIFF_STAT_TTL`].
    fn needs_run(&self, dir: &Path, now: Instant) -> bool {
        if self.dir != dir {
            return true;
        }
        let fresh = self
            .checked
            .is_some_and(|checked| now.duration_since(checked) < DIFF_STAT_TTL);
        !self.running && !fresh
    }

    /// Start a run for `dir` and return its generation. The last answer stays shown while it
    /// runs, unless the folder changed.
    fn start(&mut self, dir: &Path) -> u64 {
        if self.dir != dir {
            *self = Self {
                generation: self.generation,
                ..Self::new(dir.to_path_buf())
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
    /// The tab whose hover card shows, and the task repainting its lap time each second.
    clock: Option<(u64, Task<()>)>,
}

impl TabCards {
    /// Forget tab `id`'s `+N`: the tab is gone, or runs another session.
    pub fn forget(&mut self, id: u64) {
        self.diff_stats.remove(&id);
    }

    /// Start a `git diff` run for tab `id` in `dir`, unless one is going or answered recently.
    /// Returns the run's generation.
    fn start_diff_stat(&mut self, id: u64, dir: &Path, now: Instant) -> Option<u64> {
        let stat = self
            .diff_stats
            .entry(id)
            .or_insert_with(|| DiffStat::new(dir.to_path_buf()));
        stat.needs_run(dir, now).then(|| stat.start(dir))
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

    /// Lines added in `dir` for tab `id`, once `git diff` answered there.
    fn lines_added(&self, id: u64, dir: &Path) -> Option<u64> {
        self.diff_stats
            .get(&id)
            .filter(|stat| stat.dir == dir)
            .and_then(|stat| stat.added)
    }
}

impl WorkspaceView {
    /// The tab's details in a card next to the row, after hovering it for a moment. Opening it
    /// runs `git diff` for the `+N` (cached for a few seconds) and, while it shows, ticks the
    /// lap time of a working agent. No card while a tab is dragged. The card is built only
    /// while it shows.
    pub(super) fn with_hover_card(
        &self,
        row: AnyElement,
        id: u64,
        cx: &Context<Self>,
    ) -> AnyElement {
        if cx.has_active_drag() {
            return row;
        }
        let workspace = cx.entity().downgrade();
        let source = workspace.clone();
        HoverCard::new(("tab-card", id as usize))
            .anchor(Anchor::LeftCenter)
            .ml_2()
            .trigger(row)
            .content(move |_, _, cx| {
                let card = source
                    .upgrade()
                    .and_then(|workspace| workspace.read(cx).tab_card(id, cx));
                match card {
                    Some(card) => sidebar::tab_card(card, cx).into_any_element(),
                    None => div().into_any_element(),
                }
            })
            .on_open_change(move |open, _, cx| {
                let _ = workspace.update(cx, |workspace, cx| {
                    if *open {
                        workspace.refresh_diff_stat(id, cx);
                    }
                    workspace.set_card_clock(id, *open, cx);
                });
            })
            .into_any_element()
    }

    fn tab_card(&self, id: u64, cx: &App) -> Option<TabCard> {
        let index = self.tab_index(id)?;
        let tab = &self.tabs[index];
        let view = tab.view.read(cx);
        let location = view.location();
        let now = Instant::now();
        Some(TabCard {
            index,
            status: view.tab_status(),
            working_for: view.working_for(now),
            running_for: view.running_for(now),
            running_label: view.running_label().map(|label| label.to_string().into()),
            folder: location
                .map(|location| display_path(&location.dir, self.home.as_deref()).into()),
            branch: location
                .and_then(|location| location.head.as_ref())
                .map(|head| head.name().to_string().into()),
            title: view.name().to_string().into(),
            kind: view.kind(),
            kind_label: launcher::default_name(&tab.spec).into(),
            lines_added: location.and_then(|location| self.cards.lines_added(id, &location.dir)),
        })
    }

    /// Start or stop repainting the lap time or running time in the tab's hover card. While
    /// the card shows, the window repaints on each whole second of the time, and only while
    /// the agent is working or a command runs; otherwise the task just checks once a second
    /// whether one started. Closing a card stops the task, so nothing runs while no card
    /// shows.
    fn set_card_clock(&mut self, id: u64, open: bool, cx: &mut Context<Self>) {
        if !open {
            if self
                .cards
                .clock
                .as_ref()
                .is_some_and(|(shown, _)| *shown == id)
            {
                self.cards.clock = None;
            }
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
        let Some(location) = self
            .tab_index(id)
            .and_then(|index| self.tabs[index].view.read(cx).location())
            .filter(|location| location.head.is_some())
        else {
            return;
        };
        let dir = location.dir.clone();
        let Some(generation) = self.cards.start_diff_stat(id, &dir, Instant::now()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let added = cx.background_spawn(async move { lines_added(&dir) }).await;
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
mod tests {
    use super::*;

    #[test]
    fn diff_stat_is_reused_for_the_same_folder_only_briefly() {
        let repo = Path::new("/repo");
        let mut stat = DiffStat::new(repo.to_path_buf());
        let now = Instant::now();
        assert!(stat.needs_run(repo, now));
        let generation = stat.start(repo);
        assert!(stat.finish(generation, Some(3), now));

        assert!(!stat.needs_run(repo, now + Duration::from_secs(1)));
        assert!(stat.needs_run(repo, now + DIFF_STAT_TTL));
        assert!(stat.needs_run(Path::new("/other"), now));
    }

    #[test]
    fn diff_stat_runs_once_at_a_time_per_folder() {
        let repo = Path::new("/repo");
        let mut stat = DiffStat::new(repo.to_path_buf());
        let now = Instant::now();
        let generation = stat.start(repo);
        // However long git takes, opening the card again does not start another one.
        assert!(!stat.needs_run(repo, now + DIFF_STAT_TTL * 10));
        assert!(stat.finish(generation, Some(3), now));
        // A later run keeps showing the last answer until it has its own.
        stat.start(repo);
        assert_eq!(stat.added, Some(3));
    }

    #[test]
    fn diff_stat_ignores_an_answer_for_a_folder_left_meanwhile() {
        let mut stat = DiffStat::new(PathBuf::from("/repo"));
        let now = Instant::now();
        let old = stat.start(Path::new("/repo"));
        let other = Path::new("/other");
        assert!(stat.needs_run(other, now));
        let new = stat.start(other);
        assert_eq!(stat.added, None);

        assert!(!stat.finish(old, Some(3), now));
        assert_eq!(stat.added, None);
        assert!(stat.running);
        assert!(stat.finish(new, Some(7), now));
        assert_eq!((stat.dir.as_path(), stat.added), (other, Some(7)));
    }

    #[test]
    fn cards_show_lines_added_only_for_the_folder_they_were_counted_in() {
        let mut cards = TabCards::default();
        let (repo, now) = (Path::new("/repo"), Instant::now());
        let generation = cards.start_diff_stat(1, repo, now).unwrap();
        assert_eq!(cards.start_diff_stat(1, repo, now), None);
        assert!(cards.finish_diff_stat(1, generation, Some(4), now));

        assert_eq!(cards.lines_added(1, repo), Some(4));
        assert_eq!(cards.lines_added(1, Path::new("/other")), None);
        cards.forget(1);
        assert_eq!(cards.lines_added(1, repo), None);
    }
}
