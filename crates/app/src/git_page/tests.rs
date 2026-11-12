use super::*;
use workspace_model::git::FileDiff;

#[test]
fn only_index_and_head_count_in_the_git_folder() {
    let git_dir = Path::new("/repo/.git");
    assert!(changes_status(Path::new("/repo/.git/index"), git_dir));
    assert!(changes_status(Path::new("/repo/.git/HEAD"), git_dir));
    assert!(!changes_status(Path::new("/repo/.git/index.lock"), git_dir));
    assert!(!changes_status(Path::new("/repo/.git/logs/HEAD"), git_dir));
    assert!(!changes_status(git_dir, git_dir));
    assert!(changes_status(Path::new("/repo/src/main.rs"), git_dir));
}

#[test]
fn splits_a_path_into_name_and_folder() {
    assert_eq!(split_path("src/git/page.rs"), ("page.rs", "src/git"));
    assert_eq!(split_path("README.md"), ("README.md", ""));
}

#[test]
fn watches_the_git_and_top_folders_and_those_of_changed_files() {
    let root = std::env::temp_dir();
    let repo = RepoEntry {
        root: root.clone(),
        git_dir: root.join(".git"),
        name: "repo".into(),
    };
    let missing = "no-such-folder-for-the-git-page/a.txt";
    let status = RepoStatus {
        files: vec![
            FileStatus {
                path: "top.txt".into(),
                orig_path: None,
                staged: None,
                unstaged: Some(Change::Modified),
            },
            FileStatus {
                path: missing.into(),
                orig_path: None,
                staged: Some(Change::Deleted),
                unstaged: None,
            },
        ],
        ..RepoStatus::default()
    };

    let folders = watched_folders(&repo, Some(&status));

    // The top folder holds `top.txt`; a deleted file's folder that is gone isn't watched.
    assert_eq!(folders, HashSet::from([root.clone(), root.join(".git")]));
}

/// A new repository in a temp folder with a local identity; `None` when git is missing.
pub(super) fn temp_repo(name: &str) -> Option<PathBuf> {
    let folder = std::env::temp_dir().join(format!("agentws-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&folder)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !git(&["init", "-q", "-b", "main"]) {
        return None;
    }
    assert!(git(&["config", "user.name", "test"]));
    assert!(git(&["config", "user.email", "test@example.com"]));
    std::fs::write(folder.join("note.txt"), "hello\n").unwrap();
    Some(folder)
}

#[gpui::test]
fn lists_and_stages_the_changes_of_a_tab_repository(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    let Some(folder) = temp_repo("page") else {
        eprintln!("git not available, skipping");
        return;
    };
    cx.update(gpui_kit::init);
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let page = cx.new(|cx| GitPage::new(window, cx));
        *capture.borrow_mut() = Some(page.clone());
        Root::new(page, window, cx)
    });
    let page = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let outside = std::env::temp_dir();
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.show(vec![outside, folder.join(".")], window, cx)
        });
    });
    cx.run_until_parked();

    let state = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let page = page.read(cx);
            let status = page.status().expect("status read").clone();
            (
                page.repo_name().map(str::to_string),
                page.rows.clone(),
                status,
            )
        })
    };
    let (name, rows, status) = state(&mut cx);
    let expected_name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, Some(expected_name));
    assert_eq!(status.files[0].unstaged, Some(Change::Untracked));
    assert_eq!(
        rows,
        [
            StatusRow::Header {
                side: DiffSide::Unstaged,
                count: 1
            },
            StatusRow::File {
                side: DiffSide::Unstaged,
                index: 0
            },
        ]
    );

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.toggle_file(DiffSide::Unstaged, 0, window, cx)
        });
    });
    cx.run_until_parked();
    let (_, rows, status) = state(&mut cx);
    assert_eq!(status.files[0].staged, Some(Change::Added));
    assert_eq!(
        rows[0],
        StatusRow::Header {
            side: DiffSide::Staged,
            count: 1
        }
    );

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.toggle_group_files(DiffSide::Staged, window, cx)
        });
    });
    cx.run_until_parked();
    let (_, _, status) = state(&mut cx);
    assert_eq!(status.files[0].unstaged, Some(Change::Untracked));
    assert!(!status.has_staged());

    page.update(&mut cx, |page, _| page.hide());
    assert!(page.read_with(&cx, |page, _| page.watched.is_empty()));
    let _ = std::fs::remove_dir_all(&folder);
}

#[gpui::test]
fn a_change_asked_before_another_repository_shows_stays_in_its_own(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    // Two repositories with the same untracked `note.txt`, as two worktrees would have.
    let (Some(first), Some(second)) = (temp_repo("first"), temp_repo("second")) else {
        eprintln!("git not available, skipping");
        return;
    };
    cx.update(gpui_kit::init);
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let page = cx.new(|cx| GitPage::new(window, cx));
        *capture.borrow_mut() = Some(page.clone());
        Root::new(page, window, cx)
    });
    let page = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let folders = vec![first.clone(), second.clone()];
    cx.update(|window, cx| page.update(cx, |page, cx| page.show(folders, window, cx)));
    cx.run_until_parked();

    // Picking the second repository starts reading it; Space on the first one's file, still
    // listed, is asked before that read ends.
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            let second = page.repos[1].clone();
            page.enqueue(Op::Choose(second), window, cx);
            assert!(page.running);
            page.toggle_file(DiffSide::Unstaged, 0, window, cx);
        })
    });
    cx.run_until_parked();

    let shown = cx.update(|_, cx| page.read(cx).repo.clone().unwrap().name);
    assert_eq!(shown, second.file_name().unwrap().to_string_lossy());
    let note = |folder: &Path| git::read_status(folder).unwrap().files[0].clone();
    assert_eq!(note(&first).staged, Some(Change::Added));
    assert_eq!(note(&second).staged, None);
    assert_eq!(note(&second).unstaged, Some(Change::Untracked));

    page.update(&mut cx, |page, _| page.hide());
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

/// Run git in `folder` with a local identity; panics when it fails.
pub(super) fn run_git(folder: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(folder)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// The shown diff: its side, path and result.
type Shown = Option<(DiffSide, String, Result<FileDiff, String>)>;

#[gpui::test]
fn shows_the_diff_of_the_selected_file(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;
    use workspace_model::git::LineKind;

    let Some(folder) = temp_repo("diff") else {
        eprintln!("git not available, skipping");
        return;
    };
    std::fs::write(folder.join("mode.sh"), "x\n").unwrap();
    std::fs::write(folder.join("bin.dat"), b"a\0b").unwrap();
    run_git(&folder, &["add", "."]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    std::fs::write(folder.join("note.txt"), "hello\nworld\n").unwrap();
    std::fs::write(folder.join("bin.dat"), b"a\0c").unwrap();
    std::fs::write(folder.join("new.txt"), "one\ntwo\n").unwrap();
    run_git(&folder, &["update-index", "--chmod=+x", "mode.sh"]);

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::settings_file::init_in_memory(cx);
        init(cx);
    });
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let page = cx.new(|cx| GitPage::new(window, cx));
        *capture.borrow_mut() = Some(page.clone());
        Root::new(page, window, cx)
    });
    let page = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        page.update(cx, |page, cx| page.show(vec![folder.clone()], window, cx));
    });
    cx.run_until_parked();

    let select = |cx: &mut VisualTestContext, files: &[(DiffSide, &str)]| {
        cx.update(|window, cx| {
            page.update(cx, |page, cx| {
                for (side, path) in files {
                    page.select((*side, path.to_string()), window, cx);
                }
            })
        });
        cx.run_until_parked();
    };
    let shown = |cx: &mut VisualTestContext| -> Shown {
        cx.update(|_, cx| {
            let diff = page.read(cx).diff.as_ref()?;
            Some((diff.side(), diff.path().to_string(), diff.result().clone()))
        })
    };
    let lines = |shown: Shown| -> Vec<(LineKind, String)> {
        let Some((_, _, Ok(FileDiff::Text(diff)))) = shown else {
            panic!("not a text diff: {shown:?}");
        };
        diff.hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .map(|line| (line.kind, line.text().into_owned()))
            .collect()
    };
    assert!(shown(&mut cx).is_none());

    // 1. A modified file shows its unstaged diff.
    select(&mut cx, &[(DiffSide::Unstaged, "note.txt")]);
    let diff = shown(&mut cx);
    assert_eq!(
        diff.as_ref().map(|(side, path, _)| (*side, path.as_str())),
        Some((DiffSide::Unstaged, "note.txt"))
    );
    assert_eq!(
        lines(diff),
        [
            (LineKind::Context, "hello".to_string()),
            (LineKind::Added, "world".to_string())
        ]
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // `debug_bounds` needs debug assertions: release test runs skip the drawn checks.
    if cfg!(debug_assertions) {
        assert!(cx.debug_bounds("git-diff").is_some());
        assert!(cx.debug_bounds("git-diff-hunk-0").is_some());
    }

    // 2. Staged, the file leaves the selection to the row at its place; selected in the
    // Staged group, it shows its staged diff.
    let index = cx.update(|_, cx| {
        let status = page.read(cx).status().unwrap().clone();
        status
            .files
            .iter()
            .position(|f| f.path == "note.txt")
            .unwrap()
    });
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.toggle_file(DiffSide::Unstaged, index, window, cx)
        });
    });
    cx.run_until_parked();
    let (selected, diff) = cx.update(|_, cx| {
        let page = page.read(cx);
        let diff = page.diff.as_ref().map(|d| (d.side(), d.path().to_string()));
        (page.selected.clone(), diff)
    });
    assert_ne!(selected, Some((DiffSide::Unstaged, "note.txt".to_string())));
    assert_eq!(diff, selected);
    select(&mut cx, &[(DiffSide::Staged, "note.txt")]);
    let diff = shown(&mut cx);
    assert_eq!(
        diff.as_ref().map(|(side, ..)| *side),
        Some(DiffSide::Staged)
    );
    assert_eq!(lines(diff)[1], (LineKind::Added, "world".to_string()));

    // 3. An untracked file shows as all added.
    select(&mut cx, &[(DiffSide::Unstaged, "new.txt")]);
    assert_eq!(
        lines(shown(&mut cx)),
        [
            (LineKind::Added, "one".to_string()),
            (LineKind::Added, "two".to_string())
        ]
    );

    // 4. A binary file and a mode-only change.
    select(&mut cx, &[(DiffSide::Unstaged, "bin.dat")]);
    assert!(matches!(shown(&mut cx), Some((_, _, Ok(FileDiff::Binary)))));
    select(&mut cx, &[(DiffSide::Staged, "mode.sh")]);
    let message = cx.update(|_, cx| page.read(cx).diff.as_ref().unwrap().message());
    assert_eq!(
        message.as_deref(),
        Some("No changes in the content (mode or rename only).")
    );

    // 5. Two files selected in a row: only the second's diff shows.
    select(
        &mut cx,
        &[
            (DiffSide::Unstaged, "bin.dat"),
            (DiffSide::Unstaged, "new.txt"),
        ],
    );
    let diff = shown(&mut cx);
    assert_eq!(diff.map(|(_, path, _)| path), Some("new.txt".to_string()));

    // Tab moves the keyboard into the diff, Escape back to the list.
    let focus = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let page = page.read(cx);
            (
                page.focus.is_focused(window),
                page.diff_focus.is_focused(window),
            )
        })
    };
    cx.update(|window, cx| window.focus(&page.read(cx).focus.clone(), cx));
    cx.simulate_keystrokes("tab");
    assert_eq!(focus(&mut cx), (false, true));
    cx.simulate_keystrokes("pagedown home escape");
    assert_eq!(focus(&mut cx), (true, false));
    // Shift+Tab goes back from the list to the commit message, then around to the diff.
    cx.simulate_keystrokes("shift-tab shift-tab");
    assert_eq!(focus(&mut cx), (false, true));
    cx.simulate_keystrokes("shift-tab");
    assert_eq!(focus(&mut cx), (true, false));

    page.update(&mut cx, |page, _| page.hide());
    let _ = std::fs::remove_dir_all(&folder);
}

#[gpui::test]
fn a_commit_keeps_the_selected_unstaged_file(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    let Some(folder) = temp_repo("commit-selected") else {
        eprintln!("git not available, skipping");
        return;
    };
    run_git(&folder, &["add", "note.txt"]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    std::fs::write(folder.join("a.txt"), "a\n").unwrap();
    std::fs::write(folder.join("b.txt"), "b\n").unwrap();
    run_git(&folder, &["add", "a.txt"]);

    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::settings_file::init_in_memory(cx);
        init(cx);
    });
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let page = cx.new(|cx| GitPage::new(window, cx));
        *capture.borrow_mut() = Some(page.clone());
        Root::new(page, window, cx)
    });
    let page = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let folders = vec![folder.clone()];
    cx.update(|window, cx| page.update(cx, |page, cx| page.show(folders, window, cx)));
    cx.run_until_parked();

    // The selected file is the status's second; the commit leaves it the only one, so the old
    // rows index past the new status.
    let b = (DiffSide::Unstaged, "b.txt".to_string());
    cx.update(|window, cx| page.update(cx, |page, cx| page.select(b.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            let target = page.target().unwrap();
            page.committing = true;
            page.enqueue(Op::Commit(target, "add a".to_string()), window, cx);
        })
    });
    cx.run_until_parked();

    let (rows, selected) = cx.update(|_, cx| {
        let page = page.read(cx);
        (page.rows.clone(), page.selected.clone())
    });
    assert_eq!(
        rows,
        [
            StatusRow::Header {
                side: DiffSide::Unstaged,
                count: 1
            },
            StatusRow::File {
                side: DiffSide::Unstaged,
                index: 0
            },
        ]
    );
    assert_eq!(selected, Some(b));

    page.update(&mut cx, |page, _| page.hide());
    let _ = std::fs::remove_dir_all(&folder);
}
