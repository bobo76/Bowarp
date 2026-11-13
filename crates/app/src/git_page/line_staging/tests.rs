use std::path::{Path, PathBuf};

use gpui_kit::component::{Root, WindowExt};
use gpui_kit::{AppContext, Entity, TestAppContext, VisualTestContext};
use workspace_model::git::{Change, DiffSide, LineRef};

use super::*;
use crate::git_page::Op;
use crate::git_page::tests::{run_git, temp_repo};

const FILE: &str = "code.txt";
const COMMITTED_LINES: usize = 20;

/// `l1` .. `l20`, with `edit` applied to the lines (as a list of strings).
fn content(edit: impl FnOnce(&mut Vec<String>)) -> String {
    let mut lines: Vec<String> = (1..=COMMITTED_LINES).map(|n| format!("l{n}")).collect();
    edit(&mut lines);
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// `l2` replaced by `L2` (a mixed hunk) and `new` added after `l18` (a second hunk).
fn edited() -> String {
    content(|lines| {
        lines[1] = "L2".into();
        lines.insert(18, "new".into());
    })
}

/// A repository with `code.txt` committed as `l1` .. `l20`, then [`edited`] in the working
/// tree. `None` when git is missing.
fn repo(name: &str) -> Option<PathBuf> {
    let folder = temp_repo(name)?;
    std::fs::write(folder.join(FILE), content(|_| {})).unwrap();
    run_git(&folder, &["add", "."]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    std::fs::write(folder.join(FILE), edited()).unwrap();
    Some(folder)
}

/// The content of `path` in the index.
fn index_content(folder: &Path, path: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["show", &format!(":{path}")])
        .current_dir(folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn sign(kind: LineKind) -> char {
    match kind {
        LineKind::Added => '+',
        LineKind::Removed => '-',
        LineKind::Context => ' ',
    }
}

struct Harness {
    page: Entity<GitPage>,
    cx: VisualTestContext,
}

impl Harness {
    fn open(cx: &mut TestAppContext, folder: &Path) -> Self {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings_file::init_in_memory(cx);
            super::super::init(cx);
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let capture = captured.clone();
        let window = cx.add_window(move |window, cx| {
            let page = cx.new(|cx| GitPage::new(window, cx));
            *capture.borrow_mut() = Some(page.clone());
            Root::new(page, window, cx)
        });
        let page = captured.borrow().clone().unwrap();
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let folders = vec![folder.to_path_buf()];
        cx.update(|window, cx| page.update(cx, |page, cx| page.show(folders, None, window, cx)));
        cx.run_until_parked();
        Self { page, cx }
    }

    fn update<R>(
        &mut self,
        f: impl FnOnce(&mut GitPage, &mut Window, &mut Context<GitPage>) -> R,
    ) -> R {
        let page = self.page.clone();
        let result = self
            .cx
            .update(|window, cx| page.update(cx, |page, cx| f(page, window, cx)));
        self.cx.run_until_parked();
        result
    }

    fn select(&mut self, side: DiffSide, path: &str) {
        self.update(|page, window, cx| page.select((side, path.to_string()), window, cx));
    }

    fn focus_diff(&mut self) {
        self.update(|page, window, cx| window.focus(&page.diff_focus.clone(), cx));
    }

    /// The changed lines of the diff shown, as `+text` / `-text`.
    fn changed(&mut self) -> Vec<String> {
        self.update(|page, _, _| {
            let diff = page.diff.as_ref().expect("a diff shows");
            let text = diff.text().expect("a text diff");
            diff.changed_lines()
                .iter()
                .map(|&line| {
                    let line = text.line(line).unwrap();
                    format!("{}{}", sign(line.kind), line.text())
                })
                .collect()
        })
    }

    /// The changed line of the diff shown whose `+text` / `-text` is `wanted`.
    fn line(&mut self, wanted: &str) -> LineRef {
        let index = self
            .changed()
            .iter()
            .position(|line| line == wanted)
            .unwrap_or_else(|| panic!("no line {wanted}"));
        self.update(|page, _, _| page.diff.as_ref().unwrap().changed_lines()[index])
    }

    fn cursor_text(&mut self) -> Option<String> {
        let cursor = self.update(|page, _, _| page.lines.cursor())?;
        let index = self.update(|page, _, _| page.diff.as_ref()?.line_index(cursor))?;
        self.changed().get(index).cloned()
    }

    fn notifications(&mut self) -> usize {
        self.cx.update(|window, cx| window.notifications(cx).len())
    }

    fn close(mut self, folder: &Path) {
        self.update(|page, _, _| page.hide());
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[gpui::test]
fn stages_one_line_of_a_mixed_hunk(cx: &mut TestAppContext) {
    let Some(folder) = repo("one-line") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, FILE);
    assert_eq!(h.changed(), ["-l2", "+L2", "+new"]);

    let line = h.line("+L2");
    h.update(|page, window, cx| {
        page.lines.click(line);
        page.apply_selection(DiffSide::Unstaged, window, cx);
    });

    let expected = content(|lines| lines.insert(2, "L2".into()));
    assert_eq!(index_content(&folder, FILE), expected);
    assert_eq!(h.changed(), ["-l2", "+new"]);
    // Nothing stays selected; the cursor is on the line that took the staged one's place.
    assert!(h.update(|page, _, _| page.lines.selected().is_empty()));
    assert_eq!(h.cursor_text().as_deref(), Some("+new"));
    h.close(&folder);
}

#[gpui::test]
fn stages_a_hunk_then_unstages_one_line_from_the_staged_diff(cx: &mut TestAppContext) {
    let Some(folder) = repo("hunk") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, FILE);

    h.update(|page, window, cx| page.apply_hunk(DiffSide::Unstaged, 1, window, cx));

    let staged_hunk = content(|lines| lines.insert(18, "new".into()));
    assert_eq!(index_content(&folder, FILE), staged_hunk);
    assert_eq!(h.changed(), ["-l2", "+L2"]);
    let file = h.update(|page, _, _| page.status().unwrap().files[0].clone());
    assert_eq!(
        (file.staged, file.unstaged),
        (Some(Change::Modified), Some(Change::Modified))
    );

    // Everything staged, then `+L2` unstaged from the Staged diff.
    run_git(&folder, &["add", FILE]);
    h.update(|page, window, cx| page.enqueue(Op::Refresh, window, cx));
    h.select(DiffSide::Staged, FILE);
    assert_eq!(h.changed(), ["-l2", "+L2", "+new"]);
    let line = h.line("+L2");
    h.update(|page, window, cx| {
        page.lines.click(line);
        page.apply_selection(DiffSide::Staged, window, cx);
    });

    let expected = content(|lines| {
        lines.remove(1);
        lines.insert(17, "new".into());
    });
    assert_eq!(index_content(&folder, FILE), expected);
    assert_eq!(h.changed(), ["-l2", "+new"]);
    h.close(&folder);
}

#[gpui::test]
fn an_untracked_file_stages_only_whole(cx: &mut TestAppContext) {
    let Some(folder) = repo("untracked") else {
        eprintln!("git not available, skipping");
        return;
    };
    std::fs::write(folder.join("fresh.txt"), "one\ntwo\n").unwrap();
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, "fresh.txt");
    assert!(h.update(|page, _, _| !page.diff.as_ref().unwrap().stages_by_line()));
    h.focus_diff();

    h.cx.simulate_keystrokes("down s shift-s");

    let fresh = h.update(|page, _, _| {
        let status = page.status().unwrap();
        status.files.iter().find(|f| f.path == "fresh.txt").cloned()
    });
    assert_eq!(fresh.map(|f| f.unstaged), Some(Some(Change::Untracked)));
    assert!(h.update(|page, _, _| page.lines.cursor().is_none()));
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    // `debug_bounds` needs debug assertions: release test runs skip the drawn checks.
    if cfg!(debug_assertions) {
        assert!(h.cx.debug_bounds("git-diff-hunk-0").is_some());
        assert!(h.cx.debug_bounds("git-hunk-apply-0").is_none());
    }
    h.close(&folder);
}

#[gpui::test]
fn a_stale_diff_shows_an_error_and_stages_nothing(cx: &mut TestAppContext) {
    let Some(folder) = repo("stale") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, FILE);
    let line = h.line("+L2");
    let other = content(|lines| lines[1] = "Z2".into());

    // Another program stages a different `l2` before the click: the diff on screen is stale.
    h.update(|page, window, cx| {
        std::fs::write(folder.join(FILE), &other).unwrap();
        run_git(&folder, &["add", FILE]);
        std::fs::write(folder.join(FILE), edited()).unwrap();
        page.lines.click(line);
        page.apply_selection(DiffSide::Unstaged, window, cx);
    });

    assert_eq!(h.notifications(), 1);
    assert_eq!(index_content(&folder, FILE), other);
    assert_eq!(h.changed(), ["-Z2", "+L2", "+new"]);
    h.close(&folder);
}

#[gpui::test]
fn keys_move_over_changed_lines_and_stage_the_cursor_line(cx: &mut TestAppContext) {
    let Some(folder) = repo("keys") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, FILE);
    h.focus_diff();

    // From `-l2` (after the context line `l1`) down to `+new`, past the context between.
    h.cx.simulate_keystrokes("down");
    assert_eq!(h.cursor_text().as_deref(), Some("-l2"));
    h.cx.simulate_keystrokes("down down");
    assert_eq!(h.cursor_text().as_deref(), Some("+new"));
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    // The cursor's hunk shows its button without hover (`debug_bounds` needs debug
    // assertions: release test runs skip the drawn checks).
    if cfg!(debug_assertions) {
        assert!(h.cx.debug_bounds("git-hunk-apply-1").is_some());
    }

    // `U` is for a Staged diff: nothing happens here. `S` stages the cursor's line.
    h.cx.simulate_keystrokes("u");
    h.cx.run_until_parked();
    assert_eq!(index_content(&folder, FILE), content(|_| {}));
    h.cx.simulate_keystrokes("s");
    h.cx.run_until_parked();
    assert_eq!(
        index_content(&folder, FILE),
        content(|lines| lines.insert(18, "new".into()))
    );

    // Shift+S stages the cursor's whole hunk; the file leaves Changes.
    h.cx.simulate_keystrokes("up shift-s");
    h.cx.run_until_parked();
    assert_eq!(index_content(&folder, FILE), edited());
    let selected = h.update(|page, _, _| page.selected.clone());
    assert_ne!(selected, Some((DiffSide::Unstaged, FILE.to_string())));
    h.close(&folder);
}

#[gpui::test]
fn mouse_selects_ranges_and_escape_clears_them(cx: &mut TestAppContext) {
    let Some(folder) = repo("mouse") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, &folder);
    h.select(DiffSide::Unstaged, FILE);
    let (minus, plus, new) = (h.line("-l2"), h.line("+L2"), h.line("+new"));
    let press = |h: &mut Harness, line, modifiers: Modifiers| {
        h.update(|page, window, cx| page.press_line(line, modifiers, window, cx));
    };
    let selected = |h: &mut Harness| -> Vec<LineRef> {
        h.update(|page, _, _| page.lines.selected().iter().copied().collect())
    };

    press(&mut h, minus, Modifiers::default());
    press(&mut h, new, Modifiers::shift());
    assert_eq!(selected(&mut h), [minus, plus, new]);
    press(&mut h, plus, Modifiers::secondary_key());
    assert_eq!(selected(&mut h), [minus, new]);
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    // `debug_bounds` needs debug assertions: release test runs skip the drawn checks.
    if cfg!(debug_assertions) {
        assert!(h.cx.debug_bounds("git-selection-bar").is_some());
    }

    // The first Escape clears the selection, the second gives the keyboard to the list.
    h.cx.simulate_keystrokes("escape");
    assert!(selected(&mut h).is_empty());
    assert!(h.update(|page, window, _| page.diff_focus.is_focused(window)));
    h.cx.simulate_keystrokes("escape");
    assert!(h.update(|page, window, _| page.focus.is_focused(window)));
    h.close(&folder);
}
