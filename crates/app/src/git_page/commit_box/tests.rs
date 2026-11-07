use std::path::{Path, PathBuf};

use gpui_kit::component::{Root, WindowExt};
use gpui_kit::{AppContext, Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::git_page::tests::{run_git, temp_repo};

#[test]
fn the_placeholder_names_the_branch() {
    let keys = if cfg!(target_os = "macos") {
        "Cmd+Enter"
    } else {
        "Ctrl+Enter"
    };
    assert_eq!(
        placeholder(Some("main")),
        format!("Message ({keys} to commit on main)")
    );
    assert_eq!(placeholder(None), format!("Message ({keys} to commit)"));
}

/// A repository with `note.txt` committed, then changed and staged. `None` when git is missing.
fn staged_repo(name: &str) -> Option<PathBuf> {
    let folder = temp_repo(name)?;
    run_git(&folder, &["add", "."]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    std::fs::write(folder.join("note.txt"), "hello\nworld\n").unwrap();
    run_git(&folder, &["add", "."]);
    Some(folder)
}

/// The subject of the last commit.
fn last_subject(folder: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(folder)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

struct Harness {
    page: Entity<GitPage>,
    cx: VisualTestContext,
}

impl Harness {
    fn open(cx: &mut TestAppContext, folders: Vec<PathBuf>) -> Self {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::settings_file::init_in_memory(cx);
            crate::git_page::init(cx);
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
        cx.update(|window, cx| page.update(cx, |page, cx| page.show(folders, window, cx)));
        cx.run_until_parked();
        // Drawn once, so the message input is laid out and can take the keyboard.
        cx.update(|window, cx| window.draw(cx).clear(cx));
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

    /// Give the keyboard to the message and type `text` in it.
    fn type_message(&mut self, text: &str) {
        self.update(|page, window, cx| window.focus(&page.message_focus(cx), cx));
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
    }

    fn message(&mut self) -> String {
        self.update(|page, _, cx| page.message_text(cx))
    }

    fn can_commit(&mut self) -> bool {
        self.update(|page, _, cx| page.can_commit(cx))
    }

    fn has_staged(&mut self) -> bool {
        self.update(|page, _, _| page.status().is_some_and(RepoStatus::has_staged))
    }

    /// How many notifications show (their text is git's, tested in `workspace-model`).
    fn notifications(&mut self) -> usize {
        self.cx.update(|window, cx| window.notifications(cx).len())
    }

    fn close(mut self, folders: &[PathBuf]) {
        self.update(|page, _, _| page.hide());
        for folder in folders {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

#[gpui::test]
fn ctrl_enter_commits_what_is_staged_and_empties_the_message(cx: &mut TestAppContext) {
    let Some(folder) = staged_repo("commit") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, vec![folder.clone()]);
    assert!(h.has_staged());

    h.type_message("Add world");
    assert_eq!(h.message(), "Add world");
    assert!(h.can_commit());
    h.cx.simulate_keystrokes("secondary-enter");
    h.cx.run_until_parked();

    assert_eq!(last_subject(&folder), "Add world");
    assert!(!h.has_staged());
    assert_eq!(h.message(), "");
    assert!(!h.update(|page, _, _| page.committing));
    // Git's summary shows.
    assert_eq!(h.notifications(), 1);
    h.close(&[folder]);
}

#[gpui::test]
fn a_rejecting_hook_keeps_the_message_and_the_staged_files(cx: &mut TestAppContext) {
    let Some(folder) = staged_repo("hook") else {
        eprintln!("git not available, skipping");
        return;
    };
    let hooks = folder.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'lint failed: fix it' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    run_git(&folder, &["config", "core.hooksPath", "hooks"]);
    let mut h = Harness::open(cx, vec![folder.clone()]);

    h.type_message("Blocked");
    h.update(|page, window, cx| page.commit(window, cx));

    assert_eq!(last_subject(&folder), "first");
    assert!(h.has_staged());
    assert_eq!(h.message(), "Blocked");
    assert!(!h.update(|page, _, _| page.committing));
    // Git's message (the hook's output) shows.
    assert_eq!(h.notifications(), 1);
    h.close(&[folder]);
}

#[gpui::test]
fn commit_waits_for_staged_changes_and_a_message(cx: &mut TestAppContext) {
    let Some(folder) = temp_repo("ready") else {
        eprintln!("git not available, skipping");
        return;
    };
    run_git(&folder, &["add", "."]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    std::fs::write(folder.join("note.txt"), "changed\n").unwrap();
    let mut h = Harness::open(cx, vec![folder.clone()]);

    // A change, but nothing staged.
    h.type_message("Change it");
    assert!(!h.can_commit());
    h.cx.simulate_keystrokes("secondary-enter");
    h.cx.run_until_parked();
    assert_eq!(last_subject(&folder), "first");
    // Ctrl+Enter adds no line either.
    assert_eq!(h.message(), "Change it");

    // Staged, with a blank message.
    run_git(&folder, &["add", "."]);
    h.update(|page, window, cx| {
        page.enqueue(Op::Refresh, window, cx);
        page.set_message(" \n ".to_string(), window, cx);
    });
    assert!(h.has_staged());
    assert!(!h.can_commit());

    h.update(|page, window, cx| page.set_message("Change it".to_string(), window, cx));
    assert!(h.can_commit());
    h.close(&[folder]);
}

#[gpui::test]
fn each_repository_keeps_its_draft(cx: &mut TestAppContext) {
    let (Some(first), Some(second)) = (staged_repo("draft-a"), staged_repo("draft-b")) else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, vec![first.clone(), second.clone()]);
    let choose = |h: &mut Harness, index: usize| {
        h.update(|page, window, cx| {
            let repo = page.repos[index].clone();
            page.enqueue(Op::Choose(repo), window, cx);
        });
    };

    h.type_message("For A");
    choose(&mut h, 1);
    assert_eq!(h.message(), "");
    h.type_message("For B");
    choose(&mut h, 0);
    assert_eq!(h.message(), "For A");
    choose(&mut h, 1);
    assert_eq!(h.message(), "For B");
    h.close(&[first, second]);
}

#[gpui::test]
fn makes_the_first_commit_of_a_new_repository(cx: &mut TestAppContext) {
    let Some(folder) = temp_repo("first-commit") else {
        eprintln!("git not available, skipping");
        return;
    };
    run_git(&folder, &["add", "."]);
    let mut h = Harness::open(cx, vec![folder.clone()]);
    assert!(!h.update(|page, _, _| page.status().unwrap().has_commits));

    h.type_message("First");
    h.update(|page, window, cx| page.commit(window, cx));

    assert_eq!(last_subject(&folder), "First");
    let status = h.update(|page, _, _| page.status().unwrap().clone());
    assert!(status.has_commits);
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(status.files.is_empty());
    h.close(&[folder]);
}

#[gpui::test]
fn tab_moves_from_the_message_to_the_list_and_escape_leaves_it(cx: &mut TestAppContext) {
    let Some(folder) = staged_repo("focus") else {
        eprintln!("git not available, skipping");
        return;
    };
    let mut h = Harness::open(cx, vec![folder.clone()]);
    let focus = |h: &mut Harness| {
        h.update(|page, window, cx| {
            (
                page.message_focus(cx).is_focused(window),
                page.focus.is_focused(window),
                page.diff_focus.is_focused(window),
            )
        })
    };

    h.type_message("x");
    h.cx.simulate_keystrokes("tab");
    assert_eq!(focus(&mut h), (false, true, false));
    // Tab moved on rather than indenting.
    assert_eq!(h.message(), "x");
    h.cx.simulate_keystrokes("shift-tab");
    assert_eq!(focus(&mut h), (true, false, false));
    h.cx.simulate_keystrokes("shift-tab");
    assert_eq!(focus(&mut h), (false, false, true));
    h.cx.simulate_keystrokes("tab");
    assert_eq!(focus(&mut h), (true, false, false));
    h.cx.simulate_keystrokes("escape");
    assert_eq!(focus(&mut h), (false, true, false));
    h.close(&[folder]);
}
