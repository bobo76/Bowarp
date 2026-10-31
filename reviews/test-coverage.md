# Test coverage follow-ups

Review of the `m2-sidebar` changes, focused on gaps not covered by the current
regression tests.

The follow-ups below are implemented. Actual Windows compositor appearance still
requires the manual checks in `BACKLOG.md`; the native failure paths are covered
with injected operations rather than a real DWM window.

## Highest priority: Windows Mica/DWM paths

The model tests cover backdrop transition decisions, but do not execute the
Windows DWM calls or verify their failure and retry behavior in
`crates/app/src/window_background.rs`.

- [x] Put the native DWM operations behind a small injectable boundary.
- [x] Use a fake implementation to test transitions between Mica, transparent,
  blurred and opaque backgrounds, including call order, HRESULT failures,
  partial Mica application, retry state, and replacement warnings.
- [x] Keep the existing pure transition tests; they cover a separate layer.

The native-operation regressions are in
`crates/app/src/window_background/tests.rs`.

## Improve opacity rendering assertions

`crates/app/src/workspace_view/tests.rs` searches painted quads across the
whole window for expected colors. An unrelated quad could satisfy an assertion,
so the test may not prove that the sidebar/panel is translucent while the file
viewer and Settings remain opaque.

- [x] Tie painted quads to the expected panel, viewer, and Settings bounds,
  converted to the window's paint scale.
- [x] Assert opacity at those bounds across Dark, Light and Sepia at 50% and
  100%, including selected tab/Settings rows and an explorer hover.
- [x] Reject stacked translucent fills; ignore fully transparent quads, and allow
  identical opaque fills from nested Settings surfaces.

## Copilot mode argument precedence

The mode parser and terminal behavior are tested separately, with synthetic
arguments in the terminal test. Repeated or conflicting flags do not have an
explicit precedence assertion.

- [x] Confirm the intended precedence against Copilot CLI 1.0.105's interactive
  startup in an isolated temporary folder, without submitting a model prompt.
- [x] Add parser cases for repeated and conflicting mode flags, then assert the
  resulting initial mode and permission attention behavior in `TerminalView`.

The last `--mode` value wins. `--plan --mode autopilot` starts in Plan in either
order; repeated `--plan` and `--autopilot` flags are accepted. Copilot rejects
`--autopilot` with `--mode` or `--plan`, and `--plan` with an explicit mode other
than Autopilot. Rejected combinations do not seed a mode.
