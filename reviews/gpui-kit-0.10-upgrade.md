# gpui-kit 0.7 → 0.10 upgrade review

Behavior changes from the gpui-kit 0.7.3 → 0.10.0 bump (gpui-component and gpui-base 0.10.0,
gpui-pre 0.3.9 → 0.3.12, taffy 0.13 → 0.15) that can affect the app. Found by diffing the crate
sources against the app's usage (2026-11-25); nothing below was checked in the running app.

## Fixed

- **Settings sliders collapsed to their thumb** (Opacity, Font size). taffy 0.15 uses an item's
  `w_*`/`h_*` as its flex base size only when `flex_basis` is `auto`
  (`taffy-0.15.1/src/compute/flexbox.rs:1017-1031`); `flex_1()` sets it to `0%`, which a
  content-sized parent can't resolve, so the item falls back to its content size. The slider
  sits in a content-sized `items_end` column. Fixed with `.flex_none().w_48()` in
  `settings_page.rs` `slider_field`; test
  `settings_page::tests::a_slider_keeps_its_track_width_in_a_setting_row`.
- Rule to watch for: `flex_1()` + fixed size inside a content-sized parent ignores the size.
  Percentage sizes (`w_full`, `size_full`) along a content-sized parent's main axis no longer
  resolve either. Every other `flex_1` in `crates/app/src` was checked; none is affected.

## To check or decide

**Problem 1: redraws capped at ~30 fps when unfocused or on low power.**
`gpui-pre-0.3.12/src/window.rs` ~1326-1385 (`FramePacer`) now caps every redraw at 33 ms in an
unfocused window; `low_power_frame_interval` (33 ms default) does the same in a focused window
when Windows reports low power (presumably Battery Saver). Affects terminal output streaming in
a background window and the sidebar pulse. Option: `low_power_frame_interval: None` in
`main.rs` `WindowOptions` (unverified whether that also lifts the unfocused cap).

**Problem 2: keyboard shortcut chips may no longer wrap.**
taffy 0.15 puts all items on one line when the container's width comes from its content
(`flexbox.rs:1240`). `settings_page/keyboard.rs:113-118` (`flex_wrap` chips row) sits in a
content-sized `items_end` column. Check: 3-4 shortcuts on one action, narrow window. Fix: a
`max_w` on the chips row. Same mechanism, lower risk: the file viewer search bar
(`file_viewer/search.rs:369-397`), whose parent has a fixed width.

**Problem 3: Windows scrollbars show on hover.**
`gpui-component-0.10.0/src/theme/mod.rs:147, 386-392`: with auto-hide scroll bars on, the mode
went from `Scrolling` to `Hover`; with it off, `Hover` → `Always`. Affects the settings page,
branch picker and long menus (not the app's own `scrollbar.rs`). Old behavior:
`Theme::set_scrollbar_mode(ScrollbarMode::Scrolling, cx)` after init (re-applied on a system
settings change).

**Problem 4: settings page navigation.**
- The sidebar highlight follows the scroll position (`setting/page.rs:126-155, 216-239`); the
  page row loses its highlight while a group is marked.
- Clicking the title of the page already shown folds its groups (`setting/settings.rs:198-203`).
- Escape in the search clears a non-empty query (`settings.rs:174-189`).
- A search with no match shows a "No results" panel (`settings.rs:130-152`).

**Problem 5: file viewer and Markdown preview.**
- Tabs are drawn to tab stops (`gpui-base input/base/element.rs:2668`): tab-indented files look
  wider (correct rendering; check caret placement inside indentation).
- Markdown table rows highlight on hover (`gpui-component text/mod.rs:60`); each change
  redraws. Disable through a `TextViewStyle` without row hover if unwanted.
- `set_text` clears the selection (`gpui-base text/state.rs:547`): a reloaded Markdown file
  (`file_viewer.rs` reload) loses the preview selection.
- Markdown: runs of spaces collapse, short table rows are no longer padded, reference-style
  images render (`text/format/markdown.rs`). Also affects the About dialog.
- Ctrl+Enter in a multi-line input always submits (`input/base/state.rs:2435`): the file
  editor no longer inserts a line on it (the commit box keeps its own binding).

## Minor, probably fine

- Closing a dialog plays a 250 ms exit animation (`gpui-component root.rs:426-456`);
  `has_active_dialog` ignores closing dialogs, so the app's checks are unaffected.
- Submenus flip left only when their real width overflows (`menu/popup_menu.rs:1278-1306`);
  clicking a submenu row opens it.
- Scrollable dropdowns open scrolled to the checked item (`popup_menu.rs:960-999`).
- A tooltip stays hidden after its button is clicked until the pointer leaves
  (`tooltip.rs:360-407`); moving between tooltips animates their size.
- The input caret is 1 px instead of 2 px (`gpui-base input/base/blink_cursor.rs:8`).
- Touchscreen pans arrive as scroll events (`ScrollEvent`, `ScrollSource::Touch`) and scroll the
  terminal, or send wheel sequences under mouse reporting.
- Right-click menus stop the press from reaching parents and run the element's own handler
  first (`menu/context_menu.rs:401-410`): no effect expected on tab/group rows or diff lines.
- Popover content's key context is `PopoverContent`, with Enter/Space bound to `NoAction`; the
  branch picker's own contexts sit deeper and still win.
- `Label` gets an id from its call site: a debug build may panic on duplicate sibling ids
  while a screen reader is connected (library-internal labels only).
- The cached file viewer's cache key now includes inherited opacity, and entities read during
  paint invalidate it (`view.rs:545-650`).
- Inputs: `InputEditsEvent` is emitted before each `Change`; ending an IME composition can
  emit an extra `Change`.

## Checked, no effect

Stock theme colors used by the Light/Sepia palettes; list row colors and keys; switch, alert,
link, color picker, checkbox, select, resizable; notifications timing; hover card delays; kbd
formatting; button focus/tab order; input key bindings (none shadow app shortcuts), blur and
Escape handling; main window layout, dividers, terminal element, sidebar, `uniform_list` users,
diff view; `secondary-` key parsing (already handled in `keymap.rs`).
