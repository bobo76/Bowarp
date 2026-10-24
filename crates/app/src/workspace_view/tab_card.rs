//! The hover card next to a tab row: the tab's details, the `+N` lines added from `git diff`
//! and the ticking lap or running time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::{Anchor, AnyElement, App, AppContext, Context, IntoElement, Styled, Task, div};
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
        if cx.has_active_drag() || !settings_file::settings(cx).tab_card.active() {
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
                        workspace.cards.shown = Some(id);
                        workspace.refresh_diff_stat(id, cx);
                        workspace.start_card_clock(id, cx);
                    } else {
                        workspace.stop_card_clock(id);
                        if workspace.cards.shown == Some(id) {
                            workspace.cards.shown = None;
                        }
                    }
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
mod tests {
    use super::*;

    #[gpui::test]
    fn card_settings_stop_existing_clocks_and_skip_disabled_work(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::terminal_view::{TabChanged, TerminalView};
        use gpui_kit::VisualTestContext;
        use gpui_kit::component::Root;
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::{Arc, mpsc};
        use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};

        cx.update(|cx| {
            gpui_kit::init(cx);
            settings_file::init(cx);
            crate::terminal_element::init(cx);
            settings_file::preview(cx, |settings| settings.tab_card = Default::default());
        });
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "exit 0"]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
        let spec = LaunchSpec::new(
            SessionKind::Shell,
            Some(program.into()),
            args.into_iter().map(String::from).collect(),
            Some(std::env::current_dir().unwrap()),
        );
        let (exited, exits) = mpsc::channel();
        let session = Session::spawn(
            1,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(move |event| {
                if matches!(event, SessionEvent::Exited(_)) {
                    let _ = exited.send(());
                }
            }),
        )
        .unwrap();
        exits.recv_timeout(Duration::from_secs(30)).unwrap();
        let captured = Rc::new(RefCell::new(None));
        let capture = captured.clone();
        let window = cx.add_window(move |window, cx| {
            let (_, messages) = futures::channel::mpsc::unbounded();
            let workspace = cx.new(|cx| {
                WorkspaceView::new(
                    Rc::from([]),
                    None,
                    messages,
                    Rc::new(crate::tabs_file::TabsWriter::new(false)),
                    window,
                    cx,
                )
            });
            workspace.update(cx, |workspace, cx| {
                let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
                let events =
                    std::array::from_fn(|_| cx.subscribe(&view, |_, _, _: &TabChanged, _| {}));
                workspace.tabs.push(super::super::Tab {
                    id: 1,
                    view,
                    spec,
                    meta: workspace_model::TabMeta {
                        custom_name: Some("Custom tab name".into()),
                        ..Default::default()
                    },
                    viewer: None,
                    _view_events: events,
                });
                workspace.layout.add(1, None);
                let card = workspace.tab_card(1, cx).unwrap();
                assert_eq!(card.name.as_ref(), "Custom tab name");
                assert!(card.title.is_some());
                assert!(card.status.is_some());
                assert!(card.kind.is_some());
                assert!(card.folder.is_some());
                workspace.cards.shown = Some(1);
                workspace.cards.clock = Some((
                    1,
                    cx.spawn(async |_, _| {
                        futures::future::pending::<()>().await;
                    }),
                ));
            });
            *capture.borrow_mut() = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = captured.borrow().clone().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.update(|_, cx| {
            settings_file::preview(cx, |settings| {
                settings.tab_card.status = false;
                settings.tab_card.changes = false;
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                assert!(workspace.cards.clock.is_none());
                workspace.start_card_clock(1, cx);
                workspace.refresh_diff_stat(1, cx);
                assert!(workspace.cards.clock.is_none());
                assert!(workspace.cards.diff_stats.is_empty());
                let card = workspace.tab_card(1, cx).unwrap();
                assert!(card.status.is_none());
                assert!(card.working_for.is_none());
                assert!(card.running_for.is_none());
                assert!(card.running_label.is_none());
                assert!(card.lines_added.is_none());
                assert!(card.title.is_some());
                assert!(card.kind.is_some());
            })
        });
        cx.update(|_, cx| {
            settings_file::preview(cx, |settings| {
                settings.tab_card.location = false;
                settings.tab_card.program_title = false;
                settings.tab_card.session_kind = false;
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                let fields = settings_file::settings(cx).tab_card;
                assert!(fields.enabled);
                assert!(!fields.active());
                assert!(workspace.tab_card(1, cx).is_none());
                assert!(workspace.cards.shown.is_none());
                workspace.start_card_clock(1, cx);
                workspace.refresh_diff_stat(1, cx);
                assert!(workspace.cards.clock.is_none());
                assert!(workspace.cards.diff_stats.is_empty());
            })
        });
        cx.update(|_, cx| settings_file::preview(cx, |settings| settings.tab_card.enabled = false));
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                assert!(workspace.cards.shown.is_none());
                assert!(workspace.tab_card(1, cx).is_none());
            })
        });
    }

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
        assert_eq!((stat.folder.as_path(), stat.added), (other, Some(7)));
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
