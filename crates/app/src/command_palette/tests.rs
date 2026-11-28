use super::*;

fn tab(id: u64, name: &str, group: Option<&str>) -> TabRow {
    TabRow {
        id,
        name: name.into(),
        group: group.map(String::from),
        kind: SessionKind::Shell,
        needs_attention: false,
        selected: false,
    }
}

fn action(action: &'static str, label: &str) -> ActionRow {
    ActionRow {
        action,
        label: label.into(),
        keys: None,
    }
}

fn choices(delegate: &PaletteDelegate) -> Vec<PaletteChoice> {
    delegate
        .row_paths()
        .into_iter()
        .filter_map(|ix| delegate.choice(ix))
        .collect()
}

fn delegate() -> PaletteDelegate {
    PaletteDelegate::new(
        vec![tab(1, "server", Some("Backend")), tab(2, "notes", None)],
        vec![
            action("workspace::NewTab", "New tab"),
            action(
                "workspace::ToggleExplorer",
                "Show or hide the file explorer",
            ),
        ],
    )
}

#[test]
fn an_empty_query_lists_tabs_then_actions_under_titles() {
    let delegate = delegate();
    let titles: Vec<_> = delegate
        .sections
        .iter()
        .map(|section| section.title)
        .collect();
    assert_eq!(titles, [Some(TABS_TITLE), Some(ACTIONS_TITLE)]);
    assert_eq!(
        choices(&delegate),
        [
            PaletteChoice::Tab(1),
            PaletteChoice::Tab(2),
            PaletteChoice::Action("workspace::NewTab"),
            PaletteChoice::Action("workspace::ToggleExplorer"),
        ]
    );
    assert_eq!(delegate.first_row(), Some(IndexPath::new(0)));
}

#[test]
fn without_a_tab_there_is_no_tabs_section() {
    let delegate = PaletteDelegate::new(Vec::new(), vec![action("workspace::NewTab", "New tab")]);
    let titles: Vec<_> = delegate
        .sections
        .iter()
        .map(|section| section.title)
        .collect();
    assert_eq!(titles, [Some(ACTIONS_TITLE)]);
}

#[test]
fn a_query_ranks_one_untitled_list_matching_groups_and_settings_names() {
    let mut delegate = delegate();
    delegate.sections = delegate.ranked("backend");
    assert_eq!(delegate.sections.len(), 1);
    assert_eq!(delegate.sections[0].title, None);
    assert_eq!(choices(&delegate), [PaletteChoice::Tab(1)]);
    delegate.sections = delegate.ranked("toggleexp");
    assert_eq!(
        choices(&delegate),
        [PaletteChoice::Action("workspace::ToggleExplorer")]
    );
    delegate.sections = delegate.ranked("n");
    // Word starts: "notes" (prefix) before "New tab", then the explorer's "n" inside a word.
    assert_eq!(
        choices(&delegate)[..2],
        [
            PaletteChoice::Tab(2),
            PaletteChoice::Action("workspace::NewTab")
        ]
    );
    delegate.sections = delegate.ranked("zzz");
    assert!(delegate.row_paths().is_empty());
    assert_eq!(delegate.first_row(), None);
}

#[test]
fn a_page_moves_ten_rows_and_stops_at_the_ends() {
    let actions = (0..25)
        .map(|_| action("workspace::NewTab", "New tab"))
        .collect();
    let delegate = PaletteDelegate::new(vec![tab(1, "one", None)], actions);
    let paths = delegate.row_paths();
    assert_eq!(paths.len(), 26);
    let down = delegate
        .page_from(Some(paths[0]), PageDirection::Down)
        .unwrap();
    assert_eq!(down, paths[10]);
    assert_eq!(
        delegate.page_from(Some(paths[20]), PageDirection::Down),
        Some(paths[25])
    );
    assert_eq!(
        delegate.page_from(Some(paths[12]), PageDirection::Up),
        Some(paths[2])
    );
    assert_eq!(
        delegate.page_from(Some(paths[3]), PageDirection::Up),
        Some(paths[0])
    );
    assert_eq!(
        delegate.page_from(None, PageDirection::Down),
        Some(paths[10])
    );
    let empty = PaletteDelegate::new(Vec::new(), Vec::new());
    assert_eq!(empty.page_from(None, PageDirection::Down), None);
}
