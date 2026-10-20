# Code review: FM IPL standards

Whole-project review (2026-10-20) against the FM IPL Clean Code (CC-1..CC-12) and Object
Calisthenics (OC-1..OC-9) rules, applied to Rust: Checkstyle does not exist here, GPUI view
entities are treated like Spring components for OC-8, and GPUI's `cx` and protocol names
(`csi`, `fg`/`bg`) count as conventions, not abbreviations. Trait-imposed signatures
(`Element::prepaint`/`paint`, `EntityInputHandler`) are exempt from CC-7. Line numbers are as of
commit `ca4e82e`.

Problems 1-3 (CC-5: `WorkspaceView`, `TerminalView`, and the duplicated folder watcher of
`FileExplorer`/`FileViewer`) were fixed after the review; the rest are open. Numbers are kept
from the review.

## 🔴 Need to follow

**Problem 4: 🔴 [CC-10]** Render code mutates state.
- `crates/app/src/history_popup.rs:208-269` `render_history_popup` writes `popup.rows`, resets
  `popup.query`/`popup.selection` and calls `keep_in_view`.
- `crates/app/src/history_popup.rs:196-206` `update_history` (called from `render`,
  `terminal_view.rs:1058`) sets `ghost_text` and closes the popup; also CC-2 ("refresh and
  render").
- `crates/app/src/workspace_view.rs:1531` `render` writes `self.pulse_running`; `:1542-1544`
  sets the window title.

Move the mutations to the events that cause them (prompt change, key, window activation).

**Problem 5: 🔴 [CC-10]** `crates/app/src/terminal_view.rs:420-423` `emit_tab_changed` re-reads
the child's folder and git head (`Location::read`) and mutates `location` before emitting. Rename
it (`refresh_location_and_notify`) or have callers call `refresh_location()` explicitly.

**Problem 6: 🔴 [CC-9]** Direction passed as a `bool`: `workspace_view.rs:1057`
`step_search(forward)`, `:1170` `cycle(forward)`, `workspace-model/src/search.rs:73`
`step_match(.., forward)`. Call sites read as `step_search(false, ..)`
(`sidebar_rows.rs:142,146`) and `cycle(!shift, ..)` (`workspace_view.rs:1406`).
`workspace_model::layout::Direction { Up, Down }` already exists: use it.

**Problem 7: 🔴 [CC-9]** Behaviour-switching flags:
- `workspace_view.rs:1108` `set_card_clock(id, open)`: one branch spawns a ticking task, the
  other drops it. Split into `start_card_clock` / `stop_card_clock`.
- `agent-status/src/status.rs:99` `apply_progress(shown)`: pass the `ProgressState` the caller
  has (`terminal_view.rs:441`).
- `file_viewer.rs:748` `code_editor(state, readonly)`, called with literals at `:633`/`:637`.

**Problem 8: 🔴 [CC-9]** `is_dir` (and `is_root`, `can_cd`) threaded through the explorer:
`file_explorer.rs:351` `click`, `:363` `run_command`, `:486` `confirm_delete` (the flag picks
the dialog text), `:551` `entry_menu`, `:813` `command_item`, and `file_tree.rs:206`
`entry_folder`. Introduce `EntryKind { File, Folder }` or pass the `TreeRow`.

**Problem 9: 🔴 [CC-9]** Platform flags as test seams: `launcher.rs:115`
`preferred_shell(.., prefer_git_bash)`, `workspace-model/src/wheel.rs:14`
`notch_lines(lines, linux)`, `file_tree.rs:437` `validate_name_for(name, windows)`. Keep the
testability with a `Platform` enum instead of the flag.

**Problem 10: 🔴 [CC-9]** Further flags: `sidebar.rs:265` `running_dot(.., pulse, ..)` (two
render paths; threaded through `status_badge:299` and `session_icon:349`),
`sidebar_rows.rs:357` `render_group_header(.., searching, ..)`, `file_explorer.rs:197`
`set_root(root, can_cd, ..)`, `term-core/src/prompt.rs:207` `mark(mark, in_sync)`,
`prompt.rs:579` `MarkFilter::new(.., holds_prompts)`, `terminal_view.rs:190`
`spawn(.., reports_status, ..)`, `workspace_view.rs:230` `new(.., owns_tabs, ..)`. Enums where
the flag is state (`TabOwnership::Owner/Guest`), a split where it selects a path.

**Problem 11: 🔴 [CC-11]** Magic numbers worth naming:
- `sidebar.rs` layout literals: `199-200, 208, 309-311, 323, 325, 328, 364, 367, 383, 407,
  413, 463-466, 573, 589, 664, 689, 692, 719, 854, 859, 895`. The icon tile `32.`/`18.` is
  repeated at `364/367` and `689/692`.
- A 40 px header height three times: `sidebar_rows.rs:97`, `file_explorer.rs:717`,
  `file_viewer.rs:580`. Menu width `200.`: `tab_menu.rs:71, 202`, `file_explorer.rs:565`.
- `terminal_element.rs:410` (`0.6` font fallback), `:545` (`0.6`/`0.35`), `:550` (`0.08`).
- `terminal_view.rs:203` `TermDimensions::new(80, 24)`.
- `term-core/src/palette.rs:76-93` xterm 256-color cube math (`16, 231, 36, 6, 40, 55, 232,
  10`).
- `3600`/`60` duplicated in `running.rs:37` and `lap.rs:47`; `1024 * 1024` in
  `file_view.rs:112-113`; WebP header offsets `8..12` in `file_view.rs:141`.
- `terminal_links.rs:155` `..7` (`"file://".len()`).
- `history_popup.rs:391` `220.`, `about.rs:44` `380.`, `about.rs:51` `72.`.

Not raised: the escape-sequence lookup tables (`keys.rs:122-160`, the palette's ANSI table,
`mouse.rs:50-51`) and color hex values in `match` arms, where the arm names the value.

**Problem 12: 🔴 [CC-11]** Magic strings that drift:
- `workspace_view.rs:1406-1415` matches `"tab"`, `"w"`, `"b"`, `"s"`, `"a"` inline while
  `shortcuts.rs:11-16` has `NEW_TAB_KEY`, `DUPLICATE_TAB_KEY`, `EXPLORER_KEY`.
- Shortcut text hard-coded in tooltips: `workspace_view.rs:1478,1480` (Ctrl+Shift+B),
  `sidebar.rs:511,707` (`Close (Ctrl+Shift+W)`), `terminal_view.rs:1100`,
  `sidebar_rows.rs:166-167` (Ctrl+Shift+A), although `shortcuts::with_shortcut`
  (`shortcuts.rs:32`) exists for this. Rebinding a key leaves these tooltips wrong.
- `workspace-model/src/osc133.rs:218-223` OSC 133 parameter keys (`b"aw"`, `b"cmd"`,
  `b"cwd"`, `b"cur"`, `b"buf"`, `b"post"`) next to the named `MARK_PREFIX`/`PATH_PARAM`.

**Problem 13: 🔴 [CC-2]** `crates/app/src/main.rs:76-182` `main` (about 105 lines) scrubs
inherited env vars, validates args, starts hooks, takes the instance lock, initializes GPUI,
wires quit, builds window options, restores or opens tabs, and saves on close. Extract
`scrub_inherited_agent_env`, `start_hooks`, `open_workspace_window`,
`restore_or_open_first_tab`.

**Problem 14: 🔴 [CC-2]** `crates/workspace-model/src/osc133.rs:198-263` `finish` splits the
path parameter, parses key=value pairs, checks the token, decodes hex and builds the
`PromptMark`. Split into `parse_params(after) -> Params` and `Params::into_mark(letter, path)`.

**Problem 15: 🔴 [CC-2]** `crates/app/src/terminal_links.rs:49-146` `link_at` reads the
hyperlink span under the lock, joins wrapped rows into a logical line, finds the link, resolves
it on disk, and builds rows. Extract `hyperlink_span`, `logical_line`, `path_target`.

## 🟡 Should follow

**Problem 16: 🟡 [CC-3]** 278 of 789 production functions exceed 10 lines, 63 exceed 30. GPUI
builder chains were judged by abstraction level rather than line count. The imperative ones:
- `terminal_view.rs:467-544` `on_session_event`: one method per event.
- `terminal_element.rs:564-646` `snapshot`: extract `cell_style(flags, fg)` and cursor handling.
- `workspace_view.rs:399-442` `fall_back_to_shell`.
- `workspace-model/src/layout.rs:321-363` `drop_tab`: validation, mutation and change detection
  (by cloning the whole layout, `:337`).
- `prompt.rs:207-272` `mark`; `term-core/src/session.rs:200-273` `spawn`.
- `file_viewer.rs:613-696` `render_body`: the Markdown arm into its own method.
- `file_explorer.rs:607-683` `render_row`.
- `sidebar_rows.rs:29-129` `render_sidebar`: row selection logic mixed with the panel chrome.

**Problem 17: 🟡 [CC-8]**
- `crates/app/src/shell_integration.rs:87-154` `attach` switches on `Shell` with per-shell
  setup, plus a capability check `matches!(shell, Shell::Zsh | Shell::Pwsh)` at `:145`. Move
  into `Shell::configure(spec, dir, token)` and `Shell::has_hidden_cd()`.
- `file_viewer.rs:771-804` `render_text` is an `if` chain on `ViewKind` where everything not
  Markdown/SVG falls through to JSON; `Source`/`Image` stay out only because callers check
  first. An exhaustive `match` lets the compiler enforce it.
- `workspace_view.rs:1395-1423` `handle_tab_shortcut` (cyclomatic about 13): convert to GPUI
  actions plus a `KeyBinding` table, as `terminal_view.rs:68` `default_bindings` already does.

**Problem 18: 🟡 [CC-7]**
- `terminal_element.rs:743` `push_cell` (7 args). The `(line, column, cells)` triple recurs in
  `TextBatch:55`, `BackgroundRect:73`, `CursorCell:109`, `BackgroundRects::push:86`,
  `CellGrid::bounds:436`: introduce a `CellSpan`.
- `file_explorer.rs:551` `entry_menu` (6), `:813` `command_item` (6).
- `sidebar.rs:349` `session_icon` (6): pass `&TabRow`.
- `history_popup.rs:353` `history_row` (5); `settings_page.rs:248` `bound_row` (5).

**Problem 19: 🟡 [OC-1]** Nesting: `osc133.rs:117-161` `advance` (6 levels),
`file_view.rs:221-270` `layout_json` (6, extract `copy_string_literal`), `layout.rs:321-363`
(5), `workspace_view.rs:654-681` `close_tab` (5), `workspace_view.rs:1271-1280`
`on_explorer_event` nested `match` (reuse `open_default_shell`; "No shell found" is duplicated
at `:610`/`:1276`), `file_explorer.rs:320-330` `sync_watches` (extract `real_path_of`),
`sidebar_rows.rs:49-61`.

**Problem 20: 🟡 [CC-12/OC-2]** `crates/app/src/file_explorer.rs:644-653`
`if !is_dir { .. } else { .. }`: swap the branches. Statement-form `else` that would read better
as early returns: `workspace_view.rs:437`, `:1217`, `prompt.rs:225`.

**Problem 21: 🟡 [OC-6]** One concept, three names: `cwd` (`workspace_view.rs:588, 593`,
`terminal_view.rs:136`), `folder` (`terminal_view.rs:305` and the docs), `dir`
(`Location.dir`). Settle on `folder`.

## 🟢 Follow when it doesn't hurt

**Problem 22: 🟢 [OC-4]** `crates/app/src/terminal_element.rs:64` `CellStyle` is five
`bool`s built field by field at `:627`, `:696`, `:708`; store alacritty's `Flags` bitset, or
derive `Default` and use struct update.

## Outside the rule set

**Problem 23 (DRY):**
- `(not installed)` label: `sidebar_rows.rs:476-480`, `file_explorer.rs:575-580`.
- Selected/hover background: `sidebar.rs:426-435` vs `651-660`.
- Status mark icon: `sidebar.rs:554-564` vs `780-791`.
- File name of a path, four times: `file_viewer.rs:227-230, 521-527`,
  `file_explorer.rs:404-407, 493-496`.
- "Drop the SVG image and render again": `file_viewer.rs:302-315` vs `420-431`; image check to
  content: `file_viewer.rs:233-237` vs `400-403`.
- Selected tab's group lookup: `workspace_view.rs:313-316` vs `552-555`.
- Indent formula: `file_explorer.rs:610` vs `692`.

**Problem 24 (unused code):** `crates/app/src/settings_page.rs:247, 310, 334` `bound_row`,
`switch_row`, `text_row` have no caller yet (`#[expect(dead_code)]`). Land them with the first
settings row that uses them.
