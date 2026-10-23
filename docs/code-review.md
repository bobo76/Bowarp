# Code review: FM IPL standards

## Remediation (2026-10-22)

The reviews below are historical snapshots; their line numbers and confirmation notes describe
the reviewed revisions, not the current implementation.

### Branch findings

| Problem | Resolution |
|---|---|
| 1 | A stable sibling lock serializes history reads/compaction, appends and deletion across processes. Appenders lock before opening the history file. Added subprocess and concurrent-writer regressions. |
| 2 | Terminal search, sidebar search and tab cycling use the shared `Direction` enum instead of boolean navigation flags. |
| 3-4 | Shell selection, font matching, filename validation and wheel policy use an explicit `Platform`. Case folding is only allocated on Windows. |
| 5 | Foreground suppression lives at the notification caller; transition classification only examines the old and new statuses. |
| 6-7 | Named the hook timeout in milliseconds and the group hover-card width. |
| 8 | Split terminal search rebuilding, active-match selection and reveal without changing the terminal-lock scope. |
| 9 | All settings-row helpers now accept a cohesive `SettingsField` read/write pair; dropdown choices and fallback labeling are grouped together. |
| 10 | Separated keyed slider initialization, value synchronization and rendering. |
| 11 | Extracted the viewer menu and individual action builders from the header. |

### Earlier whole-project findings

| Problem | Resolution |
|---|---|
| 4-5 | History rendering computes presentation without mutating popup state; events refresh history state. Suggestions are calculated read-only at prepaint. Window titles and pulse capability are initialized/updated outside render. Renamed location refresh-and-notify to expose its side effect. |
| 6-10 | Replaced navigation, platform, entry-kind, cd availability, hook-reporting, tab-ownership, pulse and search-display flags with named types. Progress tracking accepts `ProgressState`; editor modes use named operations. Card clocks were already split into start/stop operations. |
| 11-12 | Named the cited sizing, font fallback, scrollbar, initial terminal, palette, file-size and image-header values. Shared duration/header/menu constants already covered some original locations. Centralized shortcut keys/hints and OSC parameter keys. |
| 13-15 | Extracted startup phases, OSC parameter parsing/mark construction, and terminal hyperlink/logical-line/path resolution. |
| 16-20 | Split the cited mixed-responsibility event/render/session/prompt methods, flattened parser/explorer/workspace branches, introduced `CellSpan` and cohesive row/entry parameters, made view-kind dispatch exhaustive, and registered workspace shortcuts as GPUI actions. Layout drops detect changes without cloning the layout. Existing folder-watcher helpers already resolved the cited watch nesting. |
| 21-22 | Standardized session location naming on `folder`, retaining protocol/persistence field names. Cell styling stores the terminal attribute bitset instead of five booleans. |
| 23-24 | Reused launcher labels, filename/status/selection/group helpers and SVG refresh logic. Settings row helpers now have production callers, so the unused-code finding no longer applies. |

**Validation:** all 377 workspace tests (including 108 app tests), all-target Clippy
(`-D warnings`), formatting and the six Copilot bridge tests pass. The app regressions cover
history events/render purity, settings focus and deferred startup notifications, using
deterministic PTY event delivery in GPUI tests. Rust checks ran under WSL; native Windows
execution remains unavailable because the MSVC linker is not installed.

## Local changes review (2026-10-23)

Scope: uncommitted changes on `m2-sidebar` (`git diff HEAD`, 29 files) on top of commit
`ef2b209`. Locations below refer to that working tree.

This was a static correctness review at medium effort, not an FM IPL rule review. Only
`cargo fmt --check` was run; Clippy and the tests were not, so Problems 1 and 10 still need
confirming with `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test`.

**Files needing the most work:** `agent-status/src/status.rs` (with its `settings.rs` test,
Problems 1-3), `app/src/launcher.rs` (Problem 4), `app/src/hooks.rs` (Problem 5).

### Need to fix

**Problem 1: 🔴 [test]** `crates\agent-status\src\settings.rs:69` — A misplaced closing brace
nests `copilot_registers_only_permission_requests_without_shell_quoting` inside
`registers_quoted_command_for_every_event`, so it never runs.
> `cargo test` silently skips the Copilot hooks test. The `unnameable_test_items` lint ("cannot
> test inner items") plus dead-code warnings should fail CI's Clippy step with `-D warnings`
> (`ubuntu.yml:75`).

**Problem 2: 🔴 [correctness]** `crates\agent-status\src\status.rs:110` — `apply_progress`
stamps `last_event_at` with the time the app processed the report, not when Copilot emitted it,
so a permission hook that started slightly earlier is dropped as stale.
> Copilot clears its progress and fires the PermissionRequest hook at about the same moment. The
> hook records `at = T`; the progress report crosses the pty, I/O thread and UI thread and is
> stamped `T + ms`. `apply` ignores the hook, so the tab shows Idle while Copilot waits for
> approval.

**Problem 3: 🔴 [correctness]** `crates\app\src\terminal_view.rs:518` — Copilot's
`NeedsAttention` clears only on a new progress report, but `prompt.rs` drops repeated progress
states.
> If Copilot keeps OSC 9;4 Indeterminate while asking for permission, the user approves and
> Copilot keeps working, but no new report arrives. The tab shows "needs attention" until the
> turn ends.

**Problem 4: 🔴 [correctness]** `crates\app\src\launcher.rs:80` — `shell_words::split` follows
POSIX rules: an unquoted backslash escapes the next character, mangling Windows paths in
`claude_args`/`copilot_args`.
> `--settings C:\Users\me\s.json` becomes `C:Usersmes.json` with no error, and Claude starts with
> a missing settings file. The settings page only says "Quotes group words". Use Windows-style
> splitting on Windows, or warn on the settings page.

**Problem 5: 🔴 [resource leak]** `crates\app\src\hooks.rs:47` — The Copilot plugin folder name
contains the pid and a microsecond timestamp, gets one subfolder per Copilot tab, and is removed
only by `remove_settings` at clean shutdown.
> Every crash, kill or forced logoff leaves an `agentws-<pid>-<micros>-copilot` folder behind,
> and folders for closed tabs stay until exit. Sweep stale folders at startup and remove a tab's
> subfolder when the tab closes.

### Should fix

**Problem 6: 🟡 [efficiency]** `crates\app\src\workspace_view.rs:504` — Every
`LastCommandChanged` rewrites the whole tabs file on the UI thread.
> With many tabs and a busy build loop, each command serializes all tabs and does
> create/write/rename on the UI thread for one changed string. Debounce, or save on close.

**Problem 7: 🟡 [security, DRY]** `crates\workspace-model\src\window_state.rs:141` —
`write_private_atomic` copies `write_atomic`, and its temporary-file protection is Unix-only.
> On Windows, `export TOKEN=abc` saved as `last_command` in `tabs.json` gets default permissions
> despite the doc comment. The two write paths will drift; give `write_atomic` an optional mode
> instead.

**Problem 8: 🟡 [efficiency]** `crates\app\src\workspace_view\tab_card.rs:702` —
`update_card_settings` runs on every `AppSettings` change and restarts the card clock and the
`git diff` refresh even when no tab-card setting changed.
> Dragging the file-viewer divider with a hover card open starts a clock task and a diff-stat
> request per change. Compare old and new `tab_card` values and return early when equal.

**Problem 9: 🟡 [behavior]** `crates\app\src\terminal_view.rs:286` — `last_command_supported` is
false for PowerShell/cmd tabs, and the removed `repeats_kind_label` check lets the card name
repeat the kind label.
> A pwsh tab never records or restores its last command, which the settings and README don't
> say. In `sidebar.rs` the card shows the name "✳ Claude Code" above the kind line "Claude Code";
> the function and its test were deleted.

**Problem 10: 🟡 [formatting]** `crates\app\src\workspace_view\tab_card.rs:192` — 15 changed
files are not rustfmt-formatted, including `tab_card.rs`, `settings_page.rs`, `sidebar.rs`,
`workspace-model/src/lib.rs` and `workspace_view.rs`.
> `cargo fmt --all --check` fails on 30 hunks. Over-long lines and hand-wrapped closures make the
> field-gating code hard to review.

---

## Branch review (2026-10-22)

Scope: `m2-sidebar` against `origin/main`, plus local changes, at commit `8d0b3c9`
(version `0.2.16`). Locations below refer to that revision's code.

**Original review: blocking FM IPL findings existed.** Checkstyle does not apply to this Rust repository;
these are review-standard blockers, not build failures. This was a static review; the
concurrency interleaving in Problem 1 was not runtime-reproduced.

**Validation (2026-10-22):** each finding was re-checked against the code at `8d0b3c9`.
All 11 are confirmed. *Worth* rates value against fix cost: **High**, fix in this branch;
**Medium**, fix in this branch if cheap; **Low**, optional.

| # | Confirmed | Worth | Note |
|---|---|---|---|
| 1 | ✅ | Medium | Real race, but a correctness bug rather than CC-10 |
| 2 | ✅ | High | Fix together with earlier Problem 6 (still open) |
| 3 | ✅ | High | Same fix as earlier Problem 9 (`Platform` enum) |
| 4 | ✅ | High | Also a platform flag; same `Platform` fix |
| 5 | ✅ | High | Trivial: the guard moves to the single caller |
| 6 | ✅ | Low | The `timeout` key already names it; only the unit is missing |
| 7 | ✅ | Medium | `240.` equals `SIDEBAR_WIDTH`; name the intent |
| 8 | ✅ | High | 48-line method with a three-way `if` chain |
| 9 | ✅ | Low | Matches the existing `bound_row` family's style |
| 10 | ✅ | Medium | Extract `use_slider` keyed state; small gain |
| 11 | ✅ | High | Five nesting levels inside the header renderer |

### Need to follow

**Problem 1: 🔴 [CC-10]** `crates\workspace-model\src\history.rs:295-299,308-325` —
The new deletion operation reuses an existing check/rename race: another instance can
append after the size check, then have its command silently erased by replacement.
Coordinate append, deletion and compaction with a shared cross-process lock.
> ✅ **Confirmed · Worth: Medium.** An append between `fs::metadata` (`:295`) and `fs::rename`
> (`:299`) is lost, and on Unix so is one from an appender that opened the old inode before the
> rename. The window is microseconds wide and costs at most one history line. The bug predates
> this branch in `compact`; this branch adds a second caller. It is a correctness issue, not
> CC-10 (no hidden side effect). An advisory lock file (`history.lock`) taken by
> `append_entry`, `compact` and `remove_command` fixes both paths.

**Problem 2: 🔴 [CC-9]** `crates\term-core\src\search.rs:67-78`;
`crates\app\src\terminal_view\search.rs:79` — `forward` selects previous versus next
navigation, producing opaque `true`/`false` calls. Expose explicitly named navigation methods.
> ✅ **Confirmed · Worth: High.** Call sites are `step_terminal_search(!shift, ..)`
> (`terminal_view/search.rs:39`), `(false, ..)` `:142`, `(true, ..)` `:152`, and `step(true)`
> in `terminal_element.rs:964`. Earlier Problem 6 is still open with the same pattern
> (`tab_search.rs:136` `step_search(forward)`, `workspace_view.rs:991` `cycle(forward)`,
> `workspace-model/src/search.rs:73`). Fix them together with one shared direction enum.

**Problem 3: 🔴 [CC-9]** `crates\app\src\launcher.rs:149-154,197-201` —
New `windows` flags select platform-specific startup policies. Replace them with an
explicit platform type or named platform-specific helpers.
> ✅ **Confirmed · Worth: High.** `startup_shell_on(.., windows, ..)` and
> `automatic_shell_on(.., windows, ..)` are new test seams. `automatic_shell_on` branches on
> the flag (`:200`), and `startup_shell_on` passes a literal `true` to `preferred_shell`
> (`:154`). This extends earlier Problem 9; one `Platform` enum fixes both.

**Problem 4: 🔴 [CC-9]** `crates\workspace-model\src\settings.rs:311-330` —
`ignore_case` switches font-matching behavior. Use explicitly named exact/case-insensitive
operations or a matching strategy.
> ✅ **Confirmed · Worth: High.** The only production caller passes
> `cfg!(target_os = "windows")` (`terminal_element.rs:479`), and the tests pass `false`. It is
> the same platform seam as Problem 3, so take the `Platform` enum (or a
> `Platform::font_names_match`) instead. Minor: `requested_lower` is computed even when case
> matters.

**Problem 5: 🔴 [CC-9]** `crates\app\src\terminal_view.rs:146-154` —
`visible` combines foreground suppression with status-transition classification. Gate
foreground tabs at the caller and keep `for_change` responsible only for transitions.
> ✅ **Confirmed · Worth: High.** It has one caller (`terminal_view.rs:494`), which already
> holds `self.visible`, so the fix is a one-line guard there. The `visible = true` test case
> (`:1066`) moves to the caller or is dropped.

**Problem 6: 🔴 [CC-11]** `crates\app\copilot-extension\bridge.mjs:13` —
The helper timeout is an unnamed `5000`. Introduce a meaningful constant including its
milliseconds unit.
> ✅ **Confirmed · Worth: Low.** The literal sits under the `timeout:` option key, so only the
> unit is implicit. `const HOOK_TIMEOUT_MS = 5000;` is cheap but adds little.

**Problem 7: 🔴 [CC-11]** `crates\app\src\sidebar.rs:793` —
The new hover-card width embeds `240.` directly. Define a named group-card width constant.
> ✅ **Confirmed · Worth: Medium.** `240.` equals `SIDEBAR_WIDTH` (`sidebar.rs:24`), so it
> reads as either a coincidence or a hidden coupling. Name it (`GROUP_CARD_WIDTH`) or reuse
> `SIDEBAR_WIDTH` if the card is meant to match the sidebar.

### Should follow

**Problem 8: 🟡 [CC-3, OC-1]** `crates\term-core\src\search.rs:83-131` —
`refresh` combines match rebuilding, active-match selection and viewport navigation across
nested branches. Extract those operations, preserving their current synchronization.
> ✅ **Confirmed · Worth: High.** The method is 48 lines with a three-way `if`/`else if` for
> `current` (`:109-127`). Natural splits: `rebuild_matches(term, regex)`,
> `current_after_rebuild(previous, display_offset)`, `reveal_current(term)`. They all run on
> the `&mut Term` the caller already locked, so the locking does not change.

**Problem 9: 🟡 [CC-7]** `crates\app\src\settings_page.rs:583-590` —
New `dropdown_row` takes six arguments without a framework-imposed signature. Group row
metadata, choices and settings binding into cohesive parameters.
> ✅ **Confirmed · Worth: Low.** It is six arguments, but it follows the `bound_row`
> `(title, description, .., get, set)` shape its siblings share. Grouping only `get`/`set`
> into the existing `Binding` would help everywhere, so change the whole row family or none
> of it.

**Problem 10: 🟡 [CC-3]** `crates\app\src\settings_page.rs:517-555` —
`slider_field` mixes entity/subscription initialization, state reconciliation and rendering.
Extract initialization and synchronization; retain the fluent rendering chain.
> ✅ **Confirmed · Worth: Medium.** The keyed-state closure (`:524-541`) and the
> thumb-resync check (`:544-546`) extract cleanly (`use_slider`, `sync_slider`). The gain is
> modest: the function reads top to bottom already.

**Problem 11: 🟡 [CC-3, OC-1]** `crates\app\src\file_viewer.rs:576-610` —
The new menu closure nests state lookup, action construction and view-mode iteration inside
the header renderer. Extract a dedicated menu builder and action helpers.
> ✅ **Confirmed · Worth: High.** The closure nests five levels deep inside the builder chain:
> closure, `if let`, `for`, item, `on_click`. A
> `fn viewer_menu(viewer: WeakEntity<Self>, menu, cx) -> PopupMenu` with a `mode_item` helper
> flattens it and leaves the header readable.

---

## Earlier whole-project review (2026-10-20)

Whole-project review (2026-10-20) against the FM IPL Clean Code (CC-1..CC-12) and Object
Calisthenics (OC-1..OC-9) rules, applied to Rust: Checkstyle does not exist here, GPUI view
entities are treated like Spring components for OC-8, and GPUI's `cx` and protocol names
(`csi`, `fg`/`bg`) count as conventions, not abbreviations. Trait-imposed signatures
(`Element::prepaint`/`paint`, `EntityInputHandler`) are exempt from CC-7. Line numbers are as of
commit `ca4e82e`.

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
