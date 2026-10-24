# TODO: Six small UX improvements

Saved: 2026-10-22.

## Current status

All six improvements are implemented using the recommended defaults below. Affected
model/app tests, headless UI regressions and Clippy pass in Linux Docker. Native
Windows build and visual/manual checks remain pending (no local MSVC linker).

## Implemented behavior

- [x] Settings toggle removes its sidebar row when closing. With focus in another
  pane, the first click focuses Settings and another click closes it.
- [x] File viewer menu offers existing Save and Raw/Rendered actions. Existing
  toolbar controls remain; Save is disabled without unsaved edits.
- [x] A persistent warning identifies the requested shell, retains the startup
  error and says a fallback started. It appears only if that fallback starts.
- [x] Light theme switch persists `ui_theme` as `light` or `dark`, defaults to
  dark, applies live, and leaves terminal colors unchanged.
- [x] Group hover cards show whole-group tab counts and Working/Waiting/Done
  totals, including zeros for empty groups; no cards during tab dragging.
- [x] Hidden agent terminals (another tab or Settings shown) get silent in-app
  toasts for attention, finished turns and exits, not ordinary output/title
  updates. Click selects the tab; close dismisses without selecting; otherwise
  auto-hide. Newer notices for the same tab replace earlier ones.

## Implementation checklist

- [x] **Settings button toggles closed.** When its page is already focused,
  clicking the button closes Settings. Otherwise it opens or focuses Settings.
  Start in `crates\app\src\workspace_view\settings_pane.rs`.
- [x] **File viewer overflow menu.** Add a menu for existing Save and
  Raw/Rendered actions; do not add new viewer functionality.
  Start in `crates\app\src\file_viewer.rs`.
- [x] **Shell fallback warning.** Notify the user when shell startup fails
  and a fallback shell starts, retaining the failure reason.
  Start in `crates\app\src\main.rs` and
  `crates\app\src\workspace_view.rs`.
- [x] **Light/dark UI setting.** Persist and apply the existing GPUI UI themes
  live. Defer terminal color schemes, imports, and OS-following mode.
  Start in `crates\workspace-model\src\settings.rs`,
  `crates\app\src\settings_file.rs`, `crates\app\src\settings_page.rs`,
  and `crates\app\src\main.rs`.
- [x] **Basic group hover card.** Show tab count and Working/Waiting/Done
  totals. Defer individual tab rows and ticking timers.
  Reuse `GroupSummary` in `crates\app\src\sidebar.rs` and the existing
  hover-card pattern in `crates\app\src\workspace_view\tab_card.rs`;
  group headers are in `crates\app\src\workspace_view\sidebar_rows.rs`.
- [x] **In-app agent notification toasts.** Notify when a background agent
  needs attention or finishes, without sound or OS notifications. Trigger on
  status transitions, not every output or title update.
  Start in `crates\app\src\terminal_view.rs` and
  `crates\app\src\workspace_view.rs`.

## Verification and documentation

- [x] Inspect the current worktree before editing; preserve unrelated changes.
- [x] Add targeted regression tests and review the completed changes.
- [x] Run affected tests, formatting, and Clippy.
- [ ] Manually check focus behavior, menus, theme contrast, hover cards, fallback
  warnings, and background-only notifications on the target platform.
- [x] Update the relevant user documentation and `PLAN.md`. Its policy is to
  remove fully verified items and retain items awaiting manual checks.

The existing local Docker image `bowarp-build` and volumes `bowarp-target` and
`bowarp-registry` were used for the Linux application build, 292 affected crate
tests and Clippy. Formatting was checked for changed Rust files. The local MSVC
linker was rechecked and remains unavailable.
