use std::fs;
use std::path::Path;
use std::process::Command;

use super::super::super::tests::PanesHarness;
use super::*;

fn git(folder: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .current_dir(folder)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn file_name(item: &InUse) -> &str {
    match item {
        InUse::File(name) => name,
        InUse::Tab(_) => "(tab)",
    }
}

#[test]
fn only_the_root_working_tree_counts_not_a_nested_repository_or_linked_worktree() {
    let root = std::env::temp_dir().join(format!("agentws-in-tree-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::create_dir_all(root.join("nested")).unwrap();
    let init = git(&root, &["init", "-q", "-b", "main"])
        && fs::write(root.join("a.txt"), "a\n").is_ok()
        && git(&root, &["add", "."])
        && git(&root, &["commit", "-q", "-m", "first"])
        && git(&root.join("nested"), &["init", "-q", "-b", "main"])
        && git(&root, &["worktree", "add", "-q", "linked", "-b", "other"]);
    if !init {
        eprintln!("git not available, skipping");
        let _ = fs::remove_dir_all(&root);
        return;
    }
    let top = git::locate(&root).unwrap().root;
    let items = vec![
        (root.clone(), InUse::File("top".into())),
        (root.join("sub"), InUse::File("sub".into())),
        (root.join("nested"), InUse::File("nested".into())),
        (root.join("linked"), InUse::File("linked".into())),
        (std::env::temp_dir(), InUse::File("outside".into())),
    ];
    let kept = in_working_tree(items, &top);
    let _ = fs::remove_dir_all(&root);
    let names: Vec<&str> = kept.iter().map(file_name).collect();
    assert_eq!(names, ["top", "sub"]);
}

#[gpui::test]
fn a_busy_local_shell_tab_is_in_use_but_a_remote_one_is_not(cx: &mut gpui_kit::TestAppContext) {
    use term_core::SessionEvent;

    let mut h = PanesHarness::new("switch-busy-tab", cx);
    let workspace = h.workspace.clone();
    let count = |h: &mut PanesHarness| h.cx.update(|_, cx| workspace.read(cx).in_use(cx).len());
    assert_eq!(count(&mut h), 0, "an idle tab is not in use");

    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.tabs[0].view.update(cx, |view, cx| {
                view.revive_for_test();
                view.dispatch_test_event(SessionEvent::CommandStarted(Some("sleep 9".into())), cx);
            });
        })
    });
    h.cx.run_until_parked();
    let busy = h.cx.update(|_, cx| workspace.read(cx).in_use(cx));
    assert!(
        matches!(busy.as_slice(), [(_, InUse::Tab(_))]),
        "{}",
        busy.len()
    );

    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, _| {
            workspace.tabs[0].spec.host = Some("example".into());
        })
    });
    assert_eq!(count(&mut h), 0, "a remote tab's folder is on its host");
}
