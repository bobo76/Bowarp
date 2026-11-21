use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::Root;
use gpui_kit::{AppContext, VisualTestContext};
use workspace_model::git::SwitchPlan;

use super::super::tests::{run_git, temp_repo};
use super::super::{GitPage, init};
use super::*;

/// The page's events, as text.
fn record_events(page: &Entity<GitPage>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<String>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let record = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(page, move |_, event: &GitPageEvent, _| {
            let text = match event {
                GitPageEvent::SwitchAsked(request) => format!("asked {}", request.branch),
                GitPageEvent::Switched => "switched".to_string(),
            };
            record.borrow_mut().push(text);
        })
        .detach();
    });
    events
}

/// The picker's shown branch names, per section.
fn picker_rows(page: &Entity<GitPage>, cx: &mut VisualTestContext) -> [Vec<String>; 2] {
    cx.update(|_, cx| {
        let list = page.read(cx).branch_list.read(cx);
        let delegate = list.delegate();
        let rows = |section: usize| {
            (0..delegate.shown[section].len())
                .filter_map(|row| delegate.row(IndexPath::new(row).section(section)))
                .map(|(name, _, _)| name)
                .collect()
        };
        [rows(0), rows(1)]
    })
}

#[gpui::test]
fn the_branch_picker_filters_and_switches_only_a_clean_tree(cx: &mut gpui_kit::TestAppContext) {
    let Some(folder) = temp_repo("switch") else {
        eprintln!("git not available, skipping");
        return;
    };
    run_git(&folder, &["add", "."]);
    run_git(&folder, &["commit", "-q", "-m", "first"]);
    run_git(&folder, &["branch", "topic"]);
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
    let events = record_events(&page, &mut cx);
    let folders = vec![folder.clone()];
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.show(folders, None, window, cx);
            window.focus(&page.focus, cx);
        })
    });
    cx.run_until_parked();

    // B in the list opens the picker and reads the branches, the current one first.
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_keystrokes("b");
    cx.run_until_parked();
    assert!(page.read_with(&cx, |page, _| page.picker_open));
    assert_eq!(
        picker_rows(&page, &mut cx),
        [vec!["main".to_string(), "topic".to_string()], vec![]]
    );

    // The filter narrows the list; Space types nothing and chooses nothing.
    cx.simulate_input("top");
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(
        picker_rows(&page, &mut cx),
        [vec!["topic".to_string()], vec![]]
    );
    assert!(page.read_with(&cx, |page, _| page.picker_open));
    assert!(events.borrow().is_empty());

    // Enter chooses it: the workspace is asked, then lets it go on.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!page.read_with(&cx, |page, _| page.picker_open));
    assert_eq!(*events.borrow(), ["asked topic"]);
    let request = SwitchRequest {
        root: page.read_with(&cx, |page, _| page.repo.clone().unwrap().root),
        branch: "topic".into(),
        args: vec![
            "switch".into(),
            "--no-guess".into(),
            "--".into(),
            "topic".into(),
        ],
    };
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.start_switch(request.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(*events.borrow(), ["asked topic", "switched"]);
    let branch = |cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| page.status().unwrap().branch.clone())
    };
    assert_eq!(branch(&mut cx).as_deref(), Some("topic"));

    // An untracked file blocks a switch: from the picker (nothing asked) and when the shown
    // status is stale (nothing runs, the tabs aren't told).
    std::fs::write(folder.join("new.txt"), "new\n").unwrap();
    let notifications =
        |cx: &mut VisualTestContext| cx.update(|window, cx| window.notifications(cx).len());
    let before = notifications(&mut cx);
    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            let plan = SwitchPlan::Run(vec!["switch".into(), "main".into()]);
            page.pick_branch("main".into(), plan, window, cx);
        })
    });
    assert_eq!(
        *events.borrow(),
        ["asked topic", "switched", "asked main"],
        "the shown status was clean"
    );
    events.borrow_mut().clear();
    let main = SwitchRequest {
        args: vec!["switch".into(), "--no-guess".into(), "main".into()],
        branch: "main".into(),
        ..request
    };
    cx.update(|window, cx| page.update(cx, |page, cx| page.start_switch(main, window, cx)));
    cx.run_until_parked();
    assert!(events.borrow().is_empty());
    assert_eq!(branch(&mut cx).as_deref(), Some("topic"));
    assert!(notifications(&mut cx) > before);

    cx.update(|window, cx| {
        page.update(cx, |page, cx| {
            page.pick_branch("main".into(), SwitchPlan::Nothing, window, cx);
        })
    });
    assert!(
        events.borrow().is_empty(),
        "a dirty shown status asks nothing"
    );

    page.update(&mut cx, |page, cx| page.hide(cx));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_first_row_shown_may_be_a_remote_one() {
    let mut delegate = BranchDelegate::new(WeakEntity::new_invalid());
    let branches = Branches {
        local: vec![workspace_model::git::LocalBranch {
            name: "main".into(),
            upstream: None,
            current: true,
            elsewhere: false,
        }],
        remote: vec![workspace_model::git::RemoteBranch {
            name: "origin/feature".into(),
            reference: "refs/remotes/origin/feature".into(),
            branch: "feature".into(),
        }],
    };
    delegate.set_branches(Some(Ok(branches)));
    assert_eq!(delegate.first_row(), Some(IndexPath::new(0)));
    delegate.query = "feat".into();
    delegate.filter();
    assert_eq!(delegate.first_row(), Some(IndexPath::new(0).section(1)));
    let Some(BranchChoice::Remote(remote)) = delegate.choice(IndexPath::new(0).section(1)) else {
        panic!("the remote branch is chosen");
    };
    assert_eq!(remote.branch, "feature");
    delegate.query = "nothing".into();
    delegate.filter();
    assert_eq!(delegate.first_row(), None);
}
