# UX/UI decisions

## Keyboard page: may recording take a terminal control key (Ctrl+D, Ctrl+C, Alt+B)?
- Date: 2026-10-25, step: keymap-editor, raised by: REVIEW.md #1
- Context: workspace shortcuts match from inside the terminal, so a recorded Ctrl or Alt +
  letter silently stops reaching the shell or agent (EOF, interrupt, readline keys).
- Option A — Refuse: refuse Ctrl or Alt plus a letter alone with the warning toast already
  used for typing keys; recording continues. Pros: terminal keys can't be lost by a click;
  same rule as typing keys. Cons: blocks deliberate choices such as Ctrl+Q (still possible by
  hand in the settings file).
- Option B — Save, then warn: accept the key and show a toast that programs won't get it.
  Pros: keeps full freedom. Cons: the loss happens first and is easy to miss.
- Option C — Leave it: document it only. Pros: no work. Cons: users find out when Ctrl+D
  stops working.
- Decision: A, because a terminal key lost to one click is worse than the extra step to bind
  one by hand. Ctrl+digit, Ctrl+symbol, Ctrl+Shift/Ctrl+Alt + letter, Super and F-keys stay
  allowed. Decided by: user

## Opacity: what turns translucent below 100 %?
- Date: 2026-10-25, step: opacity-blur, raised by: Step 0
- Context: the step says "terminal background only, text stays opaque"; FEATURES.md says
  "adjustable window opacity". Programs (vim, htop, agent UIs) also color cell backgrounds.
- Option A — Terminal default background only: the grid's default background and the
  terminal view's padding take the opacity; cells with an explicit background color, the
  selection, the cursor, the scrollbar, banners and toasts, and all app chrome (title bar,
  sidebar, settings, file explorer/viewer, dividers) stay opaque. Pros: matches the step and
  the defaults of Alacritty, Kitty and WezTerm (opacity on the default background only);
  chrome and colored UIs stay readable. Cons: a full-screen program that paints its own
  background (some vim themes) shows as an opaque block.
- Option B — Whole window (Warp style): every panel background takes the opacity, text
  stays opaque. Pros: one uniform glass look; preview visible from Settings. Cons: contradicts
  the step; sidebar badges, menus and the file viewer lose contrast; every panel to retune.
- Option C — Terminal plus explicit cell backgrounds: like A, also translucent colored cells
  (Alacritty's `transparent_background_colors`). Pros: no opaque blocks under vim. Cons:
  colored status lines and selections in agent UIs get hard to read; not a common default.
- Decision: A, because it is what the step asks and the common terminal default, and keeps
  every control readable. At 100 % with no blur the window stays `Opaque` (no compositor
  work); every other element must paint an explicit opaque background, or it turns see-through
  too. Decided by: UX/UI agent

## Blur: which choices per platform, and when is it "Not available on this system"?
- Date: 2026-10-25, step: opacity-blur, raised by: Step 0
- Context: GPUI blurs on Windows (acrylic, Mica, Mica Alt) and on Wayland only through KDE's
  `org_kde_kwin_blur` protocol; X11 has no blur at all, so the step's "KDE on X11" is wrong
  (fact fix: KDE Plasma on Wayland). GPUI doesn't say whether the blur protocol exists.
- Option A — One file value, platform labels: `"window_blur": "none" | "blur" | "mica"`;
  Windows lists None / Acrylic (= "blur") / Mica, Linux lists None / Blur. Mica falls back to
  Acrylic below Windows build 22621 (GPUI does nothing there), with the caption "Mica needs
  Windows 11 22H2 or later; Acrylic is used". On Linux, available when GPUI runs on Wayland
  (`cx.compositor_name() == "Wayland"`) and `XDG_CURRENT_DESKTOP` names KDE; otherwise the
  dropdown is disabled, keeps the saved value, and the caption says "Not available on this
  system"; "mica" on Linux means Blur. Pros: a copied settings file works on both systems;
  cheap; correct on the desktops that matter. Cons: a non-KDE compositor implementing the KDE
  protocol is shown as unavailable.
- Option B — Exact Wayland check: bind the Wayland registry in the app to look for
  `org_kde_kwin_blur_manager`. Pros: exact. Cons: a second Wayland connection and new
  dependencies for one caption.
- Option C — Always enabled on Linux, caption "Works on KDE Plasma (Wayland)". Pros: no
  detection. Cons: a choice that does nothing on GNOME; contradicts the step's disabled state.
- Decision: A, because it is cheap, matches the step's disabled state and keeps one file
  format; Mica Alt is left out (barely different from Mica, not a common terminal option).
  Decided by: UX/UI agent

## Blur at 100 % opacity: disabled, hidden or implied translucency?
- Date: 2026-10-25, step: opacity-blur, raised by: Step 0
- Context: blur and Mica only show through translucent pixels, and only the terminal
  background is translucent, so at 100 % a blur choice has no visible effect.
- Option A — Disabled with a caption: at 100 % the blur dropdown is disabled, keeps its value,
  and says "Needs Opacity below 100 %"; the window stays `Opaque`. Pros: exactly the
  FEATURES.md rule for a setting that needs another; nothing silently does nothing. Cons: one
  more disabled state to build (none exists for dropdowns yet).
- Option B — Blur lowers opacity: choosing blur at 100 % sets opacity to 85 %. Pros: one
  click shows the effect. Cons: changes another setting behind the user's back.
- Option C — Allowed, no effect: Windows Terminal does this. Pros: no work. Cons: looks broken.
- Decision: A, because it reuses the existing dependent-setting rule. When both apply, "Not
  available on this system" wins over "Needs Opacity below 100 %". Decided by: UX/UI agent

## Opacity slider: range, step, label, when applied?
- Date: 2026-10-25, step: opacity-blur, raised by: Step 0
- Context: `slider_row` applies live while dragged and saves on release (font size); its
  value label is a bare number in a narrow column. Settings replaces the terminal in the main
  area, so the live change can't be seen from the Settings page itself.
- Option A — 30-100, step 5, "85%" label, live while dragged: the file stores an integer
  percent (`"window_opacity": 85`, default 100 through a newtype, values clamped to 30-100,
  shown as is). Pros: 15 clearly different positions; consistent with font size; fewer
  appearance switches while dragging. Cons: 87 % only by hand in the file.
- Option B — Step 1 % (Windows Terminal, Warp). Pros: finest control. Cons: neighbouring
  values look the same; more repaints for nothing.
- Option C — Fraction 0.3-1.0 in the file (Alacritty, Kitty). Pros: matches those configs.
  Cons: the UI says percent; two units to explain.
- Decision: A, because it matches the existing slider behavior and the percent shown in the
  UI. The description says "Applies to terminal tabs"; no preview on the Settings page
  (a tab shows it after one Ctrl+Tab). Decided by: UX/UI agent

## Opacity edge cases: light theme, X11 without a compositor, full screen?
- Date: 2026-10-25, step: opacity-blur, raised by: Step 0
- Context: terminal colors don't follow the UI theme; X11 without a compositor can't show
  alpha (GPUI then also falls back to server decorations); F11 toggles full screen.
- Option A — No special cases: the light theme changes nothing (the terminal background stays
  the terminal scheme's); on X11 without a compositor the slider stays enabled and the result
  (no see-through, probably a darker background) goes to docs/known-limitations.md; opacity
  and blur stay in full screen and maximized (Windows Terminal, Alacritty, Kitty do the same).
  Pros: no work, no guesswork detection. Cons: the X11-without-compositor user gets no hint.
- Option B — Detect and warn on X11: when `window.window_decorations()` is `Server` on X11,
  disable the slider with "Needs a compositor". Pros: explains it. Cons: that signal also
  fires for window managers without `_GTK_FRAME_EXTENTS`; rare setup.
- Option C — Force opaque in full screen. Pros: none users ask for. Cons: surprising toggle.
- Decision: A, because the 30 % floor already guards readability and a compositor-less X11
  desktop is rare and can't be detected reliably. Decided by: UX/UI agent

## Linux window shadow under a see-through window: accept, remove or ask upstream?
- Date: 2026-10-25, step: opacity-blur, raised by: agent 1
- Context: with client decorations and the window not tiled/maximized, gpui-component's
  `Root` always wraps the app in `window_border()`, whose two 0.18-alpha shadows are drawn
  under the whole window with no cutout, so the see-through area gets about 33 % black first
  (at 30 % opacity the window looks about 53 % opaque and darker). The app can't change the
  shader or skip `Root`'s border without forking gpui-component. Windows, maximized, tiled
  and full-screen windows have no such shadow.
- Option A — Accept and document: keep it in docs/known-limitations.md, with the size stated
  ("darkens the see-through background by about a third, most visible at low opacity; not
  on a maximized window"). Pros: no work, no fork; the effect only makes the window look
  more opaque, never less readable. Cons: the low-opacity look on a floating Linux window is
  darker than the slider suggests.
- Option B — Server decorations while see-through: `request_decorations(Server)` below
  100 %. Pros: no shadow. Cons: GNOME Wayland has no server decorations; elsewhere a native
  frame stacks above the app's own title bar; a window frame that changes with a slider.
- Option C — Ask upstream: an issue/PR on gpui-component to skip or cut out the shadow when
  the window background isn't opaque. Pros: the real fix, at no cost to the app. Cons: out
  of the app's control and timing; posting it is the user's call (their account).
- Decision: A now, because the app can't fix it without a fork and the result stays readable;
  the known-limitations wording should say "about a third" rather than "a little". C is
  worth doing later but needs the user (posting upstream). Decided by: UX/UI agent

## "Reset to default" on the disabled Background blur dropdown?
- Date: 2026-10-25, step: opacity-blur, raised by: agent 1
- Context: the blur dropdown is disabled at 100 % opacity ("Needs Opacity below 100 %") or
  when blur isn't available ("Not available on this system") and keeps its saved value.
  `bound_row` shows "Reset to default" whenever the value differs from the default, and the
  page reset includes the row; the tab-card piece switches, disabled by their master
  switch, already show it.
- Option A — Show it: same as every other dependent row. Pros: consistent; the only in-app
  way to clear a value copied from another machine (e.g. "mica" on Linux); the page reset and
  the row agree. Cons: an enabled button under a disabled control may look odd at first.
- Option B — Hide it while disabled. Pros: the row reads as fully inactive. Cons: the page
  reset would still change the value with no row-level hint; a special case in `bound_row`;
  inconsistent with the dependent switches.
- Option C — Hide it and drop the row from the page reset. Pros: nothing changes a setting
  you can't use. Cons: "Reset all" would leave a non-default value behind; most special-casing.
- Decision: A, because it matches the existing dependent-setting rows and keeps the row and
  page resets in agreement; resetting only edits the file, it doesn't apply blur. Decided
  by: UX/UI agent

## Windows: switching Background blur live keeps the previous backdrop (Mica/Acrylic)?
- Date: 2026-10-25, step: opacity-blur, raised by: REVIEW.md #1
- Context: below 100 %, GPUI sets Mica through `DWMWA_SYSTEMBACKDROP_TYPE` and Acrylic /
  transparency through the accent policy and never undoes the other, so Mica -> None/Acrylic
  keeps Mica and Acrylic -> Mica keeps Acrylic over it until a restart. Going through 100 %
  doesn't clear Mica either (it is only covered). Nothing of it can be seen on Linux; the app
  already calls Win32 through `windows-sys` with small `unsafe` blocks (`processes.rs`), and
  `scripts/test-windows.sh` cross-builds it.
- Option A — Reset on entering Mica + document: set `Opaque` before `MicaBackdrop` (clears
  Acrylic); leaving Mica stays broken, noted in docs/known-limitations.md. Pros: existing API,
  ~4 lines. Cons: the dropdown shows a choice that isn't applied, with no hint in the UI —
  the "looks broken" case the blur-at-100 % decision rejected.
- Option A+ — A plus a caption: on Windows, once Mica was applied this session and the
  choice (below 100 %) is no longer Mica, the caption says "Mica stays until the app
  restarts" (same caption slot and style as "Needs Opacity below 100 %"). Pros: honest; the
  condition is pure logic, unit-testable in workspace-model. Cons: session state reaching the
  Settings page; a caption about our own limitation, unlike the others (which are about the
  system or another setting); still needs a restart.
- Option B — Undo Mica ourselves: A's reset on entering Mica, and on leaving it (any other
  backdrop, including Opaque) call `DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE,
  DWMSBT_NONE)` before GPUI's call, only when Mica was applied since the window opened. Pros:
  the user sees exactly what they chose, live, like every other setting; no caption, no
  restart, no limitation entry; same pattern and crate as `processes.rs` (one feature,
  `Win32_Graphics_Dwm`, plus the HWND from the window handle). Cons: one more small Windows-
  only `unsafe` block whose effect is checked only by the manual Windows test.
- Option C — Upstream fix in GPUI's `set_background_appearance`. Pros: the real fix for
  everyone. Cons: out of our control and timing; posting is the user's call.
- Decision: B, because a live setting should apply live (Windows ranks above Linux) and the
  cost is a few lines in a pattern the app already has, while A and A+ leave the user with a
  choice that silently doesn't apply or needs a restart. The BACKLOG.md manual Windows 11 check
  includes switching None/Acrylic/Mica live at 80 %, both ways and through 100 %. If that
  check shows `DWMSBT_NONE` doesn't clear Mica, fall back to A+ with the caption above and a
  known-limitations line. C stays worth posting later (needs the user). Decided by: UX/UI agent

## Copilot Working spinner: what it looks like (no reference screenshot)?
- Date: 2026-10-25, step: copilot-anim, raised by: Step 0
- Context: BACKLOG.md asks for "a blue circular spinner ... matching the reference screenshot",
  but `docs/ui-reference/` has no such screenshot, so this entry stands in for it. The row's
  first line is `text_sm` (14 px); the attention triangle in the same slot is 13 px
  (`ATTENTION_ICON_SIZE`). Status colors are fixed hex, the same in both themes; Working is
  `TabStatus::Working.color()` = `#4c9aff` (sidebar.rs:84), already the Working badge dot.
  The Running pulse caps at 10 fps (`RUNNING_PULSE_FPS`). gpui-kit ships Lucide
  `IconName::LoaderCircle` (`icons/loader-circle.svg`, a 3/4 ring with round caps, listed in
  gpui-kit-assets-0.7.3/default-icons.txt:57) and a `Spinner` component
  (gpui-component-0.7.3/src/spinner.rs:10-75: `IconName::Loader` spokes, 0.8 s,
  `ease_in_out`, `.repeat()`, no fps cap, fixed animation id `"circle"`).
- Option A — Text glyph frames (braille/half-circle, like Claude's title). Pros: echoes
  Claude. Cons: depends on the UI font having the glyphs (Windows fallback varies); a text
  frame swap is not "a circular spinner"; can't be tinted apart from a glyph-in-title look.
- Option B — gpui-kit `Spinner` as is. Pros: zero drawing code. Cons: spokes icon, not a
  ring; ease-in-out at 0.8 s with no fps cap repaints at display rate for a whole turn
  (minutes), against the idle/perf priority; can't set `with_max_fps` or `repeat_synced`.
- Option C — `Icon::new(IconName::LoaderCircle)` rotated by the app's own `with_animation`,
  like `running_dot`. Pros: a ring with a gap reads as "working" everywhere (VS Code,
  Windows Terminal tab progress ring, Warp); the existing animation pattern; fps cap and
  synced phase under our control; vector, so crisp at any scale. Cons: a few lines of our
  own instead of the component.
- Decision: C, because it is the circular spinner asked for and keeps the 10 fps cap. Exact:
  `LoaderCircle`, `size(px(ATTENTION_ICON_SIZE))` (13 px), `text_color(TabStatus::Working
  .color())` (#4c9aff, both themes), `Animation::new(1000 ms).repeat_synced()` (linear
  easing) `.with_max_fps(RUNNING_PULSE_FPS)` (10 fps = 36 degrees a frame), mapped to
  `Icon::transform(Transformation::rotate(percentage(delta)))` (icon.rs:158), animation id
  `("working-spinner", index)`. Synced phase makes several Copilot rows repaint on the same
  frames. In the light theme #4c9aff on white is about 2.9:1, just under 3:1, the same as
  today's Working badge; acceptable because state is never color-only (shape, badge, the
  card's "Working" label). Decided by: UX/UI agent

## Copilot Working spinner with "Activity animations" off: what static indicator?
- Date: 2026-10-25, step: copilot-anim, raised by: Step 0
- Context: the switch's contract is "static indicators and no animation timers; status still
  updates" (settings_page.rs:150-160); the Running badge then shows its dot unpulsed. The
  same still state applies under software rendering (`pulse_allowed`) and GPUI's
  `reduce_motion` (which renders a repeating animation's start state).
- Option A — The same LoaderCircle, not rotating (angle 0, gap at the top right). Pros: the
  row looks the same with or without motion, only the motion goes; still reads as "in
  progress"; matches what `reduce_motion` already does; no new element. Cons: a still
  loader can look "stuck" to some users.
- Option B — A blue dot in the slot. Pros: clearly static. Cons: duplicates the Working dot
  badge on the icon two centimetres away; a third look for one status.
- Option C — Nothing in the slot. Pros: simplest. Cons: the title would shift when the
  setting changes; the row loses the cue the setting promised to keep ("status still
  updates").
- Decision: A, because it mirrors the Running badge (same element, motion removed) and
  `reduce_motion`; the Working badge and the hover card's "Working · 2m 14s" back it up.
  Built as: the plain icon without `with_animation` when `may_animate` is false. Decided by:
  UX/UI agent

## Copilot Working spinner: reserve its space or let the title shift?
- Date: 2026-10-25, step: copilot-anim, raised by: Step 0
- Context: Claude's glyph is part of its OSC title (an idle mark and working frames, always
  present), so Claude's title never shifts. The app's own precedent in that slot is the
  NeedsAttention triangle (`name_line`, sidebar.rs:434-455): 13 px, `gap_1`, added only on
  that status, so the title moves right by 17 px while it shows. The sidebar is 240 px.
- Option A — Shift: add the spinner only while Working, in the triangle's slot. Pros: same as
  the triangle; Working <-> NeedsAttention swaps two 13 px icons with no jump at all; idle
  rows keep their full width. Cons: a 17 px shift at the start and end of each turn.
- Option B — Reserve 17 px on every Copilot row. Pros: no shift ever. Cons: Copilot titles
  look indented against shell/Claude rows in the same list; 7 % of the sidebar lost on idle
  rows.
- Option C — Reserve on every row of every kind. Pros: all titles aligned, no shift. Cons:
  wastes width on all rows; shell and Claude rows never use it.
- Decision: A, because it is the existing triangle behavior and the shift only happens at
  turn boundaries, when the row is meant to draw the eye. Order on the line: spinner (or
  triangle), name, pin. Decided by: UX/UI agent

## Copilot Working spinner: selected/hovered/renamed rows, cards and other title places?
- Date: 2026-10-25, step: copilot-anim, raised by: Step 0
- Context: the spinner lives only in the sidebar row's `name_line`. Other places show the
  status: the tab hover card's status chip ("Working · 2m 14s", static dot), the group hover
  card and collapsed group header (counts, dot "not pulsing here", sidebar.rs:833), the drag
  preview. There is no tab switcher. The sidebar is the only place that renders rows, so
  "visible" means: rendered (search results included), not in a collapsed group (rows are
  skipped, sidebar_rows.rs:179), window not minimized.
- Option A — Spinner in the row only; everything else static. Pros: one timer source per
  visible Copilot row and none in popovers; cards already say "Working" in words with a
  running clock; matches the cards' "not pulsing here" rule. Cons: the card's dot doesn't
  match the row's ring.
- Option B — Also animate in the tab hover card's chip. Pros: visual echo. Cons: a second
  animation for a popover shown for seconds; the card's lap clock already moves.
- Option C — Static ring (not the dot) in the card chip too. Pros: same shape everywhere.
  Cons: changes the shared `status_mark` for every Working tab, including Claude; out of
  this step's scope.
- Decision: A. Details: selected and hovered rows keep the same 13 px #4c9aff spinner (no
  color change; the row background doesn't affect it); while renaming, the editor replaces
  the line and the spinner is hidden (as the triangle is); a custom name doesn't hide it (it
  is status, not title); the drag preview stays static (name only); a collapsed group
  shows nothing extra (its header count already says Working); a focused-out but visible
  window keeps animating; a minimized window must paint no frames — if GPUI keeps
  scheduling them on a platform, gate `may_animate` on the window being visible, or record
  it in docs/known-limitations.md. Not decided here (needs the user, product direction): a
  Claude tab with a custom name hides its title glyph and so shows no Working animation in
  the row; the spec keeps the spinner Copilot-only, so leave it as is unless the user wants
  the same spinner for a renamed Claude tab. Decided by: UX/UI agent

## Copilot spinner: how often may it redraw during a long turn?
- Date: 2026-10-25, step: copilot-anim, raised by: REVIEW.md #1
- Context: each spinner frame re-renders the whole window, the active terminal included (the
  TerminalView is not cached), for the whole Working state, unlike the 10 s Running pulse.
- Option A — Accept 10 fps, cache the terminal next. Pros: smooth. Cons: ~1-3 % of a core per
  working Copilot tab until the caching step lands.
- Option B — Cache the active TerminalView now. Pros: fixes all sidebar animations. Cons:
  large and risky as a review fix.
- Option C — Stop turning after N seconds. Pros: bounded cost. Cons: the ring then no longer
  says "working".
- Option D — Turn at 2 frames a second. Pros: 5x fewer redraws, still reads as working. Cons:
  a stepped rather than smooth turn.
- Decision: D, 2 frames a second with a 2 s turn (90° steps). Decided by: user

## Discard prompt Cancel after opening a file from the settings page: put the page back?
- Date: 2026-10-25, step: settings-viewer, raised by: REVIEW.md #1
- Context: opening a file while Settings shows ("Open settings file", explorer click) closes
  the page first (`activate`), then the hidden viewer's unsaved edits to another file trigger
  "Discard changes?". On Cancel the page stays closed and the tab (with its edited file)
  shows.
- Option A — Keep it. Pros: the prompt shows over the file whose edits it is about, so the
  user can see what they would lose; Cancel leaves them on the edited file, which is usually
  what they want to save next; the Settings row is still in the sidebar, one click back; no
  new code. Cons: Cancel does not fully undo the gesture.
- Option B — Reopen the page on Cancel (`open_at_or` on `discard_then_or`). Pros: Cancel is
  a strict undo. Cons: the prompt asks about a file the user can't see if shown first, or the
  view flips twice (page -> tab -> page); ~15 lines and a dialog test for a rare path.
- Decision: A, because seeing the file the prompt is about is worth more than an exact undo,
  and returning to Settings is one click. Decided by: UX/UI agent

## Focus after opening a file while the settings page shows
- Date: 2026-10-25, step: settings-viewer, raised by: REVIEW.md #2
- Context: closing the page via `activate` focuses the tab's terminal. Today no file-open
  path focuses the viewer: a terminal link click keeps the terminal focused, an explorer
  click leaves focus where it was (explorer rows are not focusable), and `FileViewer` has no
  focus API. settings.json opens in the Rendered view, not editable until Raw is clicked.
- Option A — Keep: focus goes to the tab's terminal (same as selecting the tab). Pros: same
  as every other way of leaving the page; no new API. Cons: typing right after "Open settings
  file" goes to the shell.
- Option B — Focus the viewer after "Open settings file" only. Pros: matches the intent "edit
  this file". Cons: the Rendered view takes no text, so typing still edits nothing until Raw
  is clicked (that click focuses the editor anyway); one path focusing the viewer while all
  others don't is inconsistent; needs a new FileViewer focus API.
- Option C — Restore the focus held before the page opened. Pros: least surprise for
  explorer clicks. Cons: needs tracking pre-page focus; the pre-page target is usually the
  terminal anyway, so it mostly equals A.
- Decision: A, because B gives no real edit-on-type win while the file opens Rendered and
  would make one open path differ from the rest; "focus the viewer when a file opens" should
  be decided once for all open paths (terminal links, explorer, settings button), not here.
  Possible backlog item, needs the user (product direction): focus the viewer on file open,
  and/or open settings.json in Raw. Decided by: UX/UI agent

## Hover card still under the cursor when a hold ends: show it at once or wait for a new hover?
- Date: 2026-10-25, step: hover-card-guard, raised by: REVIEW.md #1
- Context: since 821290e the HoverCard wrapper stays during a hold (rename, menu, dialog,
  drag) and renders invisible; when the hold ends, the card open under the cursor appears at
  once, with no hover delay, and runs `git diff` (`tab_card.rs:156-159`). During a drag the
  rows passed over open invisible cards, so the card under the drop point pops up; before
  821290e a card after a drop needed a fresh hover delay.
- Option A — Keep: show the hovered card as soon as the hold ends. Pros: no code; the card
  is "earned" by the hover that happened during the hold. Cons: a card pops up on Escape,
  Cancel, Enter or a drop with no pointer move, right where ux-review #9 wanted it gone (the
  user just finished a gesture on that row); regresses the old after-drop behaviour; spawns
  `git diff` with no user intent.
- Option B — Suppress until the row is left: when the hold ends, the card that is `shown`
  becomes `TabCards::suppressed` and stays invisible (empty content, no clock, no diff) until
  its `on_open_change(false)`; leaving and re-entering the row gives the normal delay. Group
  cards the same through their slot. Pros: matches the Windows/VS Code tooltip rule (a
  tooltip dismissed by a click or menu stays away until the pointer leaves the control);
  restores the pre-821290e drop behaviour; no surprise popup after a keyboard action; no
  `git diff` on a hold end; only public HoverCard API (`appearance`, `on_open_change`), one
  `Option<u64>`, no timer or redraw loop. Cons: a user who keeps the pointer still and wants
  the card must nudge it out of the row and back (one move; the card is never needed to
  finish the gesture).
- Option C — Re-key the HoverCard id per hold. Pros: a real reset, fresh delay without
  leaving. Cons: also resets the row's ContextMenu state keyed under that id path; fragile.
- Decision: B, because a card should answer a hover, not the end of a menu, dialog, rename
  or drag; it matches familiar tooltip behaviour and the old after-drop rule at near-zero
  cost. Edge cases: `suppressed` is cleared only by the matching id's close (ids are unique,
  so a removed tab's stale id never matches and needs no cleanup); a row that was not under
  the cursor when the hold ended is unaffected; with the card setting off nothing changes; a
  hold that starts again while suppressed keeps it suppressed. docs/known-limitations.md
  lines 128-129 ("shows once it ends while its row is still hovered") no longer apply once
  built. Decided by: UX/UI agent

## Attention toast jump key: hover tooltip, visible second line, or both?
- Date: 2026-10-25, step: toast-kind, raised by: agent 1
- Context: gpui-component's `Notification` has no tooltip API; agent 1 overlays an empty
  `absolute().inset_0()` div with `.tooltip()` through `.content()`, covering the title and
  message column (not the icon or close button). The key comes from the live keymap; nothing
  when unbound. docs/ux-review.md #13 (the step as planned) asked for a tooltip.
- Option A — Tooltip only (as built): Pros: matches the planned step and the app's habit of
  putting keys in tooltips (sidebar attention button, title bar buttons); toast stays two lines
  and the same height as every other toast; unbound key simply shows nothing. Cons: keyboard
  users who never hover don't see it; the toast may hide before anyone hovers.
- Option B — Visible second line ("Ctrl+Shift+A: next tab needing attention"): Pros: seen
  without the mouse. Cons: every attention toast grows a third line of repeated text, noisy
  with several agents; inconsistent with the other toasts; contradicts the planned step; the
  key is already taught by the sidebar button's tooltip and README.
- Option C — Both: Cons of B, plus the tooltip would repeat the visible line.
- Decision: A, because it is what the step planned, costs nothing on screen and follows the
  app's "key in the tooltip" pattern; the overlay not covering icon/close is fine (the text is
  where the pointer rests). Decided by: UX/UI agent

## Attention toast tooltip wording
- Date: 2026-10-25, step: toast-kind, raised by: agent 1
- Context: built text is "Jump to tabs needing attention (Ctrl+Shift+A)", while a click on the
  toast opens that toast's tab and the key goes to the next tab needing attention after the
  selected one (README shortcut row: "Go to the next tab needing attention"; sidebar button:
  "1 tab needs attention: go to it" / "N tabs need attention: go to the next", key appended by
  `shortcuts::with_shortcut`).
- Option A — Keep "Jump to tabs needing attention (keys)": Pros: no change. Cons: "Jump" and
  the plural are used nowhere else; reads as if it described the click.
- Option B — "Go to the next tab needing attention (keys)": Pros: same verb and phrase as the
  README row and the sidebar button; "next" makes clear it is the key's cycle, not this toast;
  build it with `shortcuts::with_shortcut`'s "label (keys)" format. Cons: doesn't mention the
  click.
- Option C — "Click to open this tab; Ctrl+Shift+A: next tab needing attention": Pros: explains
  both. Cons: long for a tooltip; no other app tooltip says "Click to"; toast click is the
  convention users already expect (VS Code, Windows notifications).
- Decision: B, because it reuses the app's existing wording for this action exactly and avoids
  suggesting the key targets this toast's tab. Decided by: UX/UI agent

## Attention toast text when the wait reason changes while the tab already needs attention
- Date: 2026-10-25, step: toast-kind, raised by: agent 1 (note)
- Context: a tab going from "Waiting for permission" to "Waiting for your reply" without
  leaving NeedsAttention sends no new toast; the toast shown keeps its first text.
- Option A — Keep (as built): Pros: no second toast for one wait; the first reason is usually
  still true (a pending permission prompt is still pending); toasts hide on their own. Cons:
  the shown text can be stale for a few seconds.
- Option B — Re-toast on reason change (replaces the tab's toast, keyed per tab): Pros: always
  current. Cons: a second pop-up for the same wait, re-arming the hide timer; more noise.
- Decision: A, because a toast is a one-off nudge, not a live status; the sidebar keeps the
  live state. Decided by: UX/UI agent

## Explorer button and Ctrl+Shift+| while the settings page hides the explorer
- Date: 2026-10-26, step: settings-hides-explorer, raised by: Step 0
- Context: this step hides the explorer panel and its divider while the settings page
  shows, and restores them (open, width, expanded folders) on leaving the page. The title
  bar button (`explorer_toggle`, `.selected(self.explorer_visible)`) and `ToggleExplorer`
  (Ctrl+Shift+|, captured at the root) still exist then. A tab always exists (closing the
  last tab closes the window), so "leave the page" is `activate(self.active)`, which keeps the
  Settings row.
- Option A — Show it: leave the page and show the explorer. While the page shows, the button
  reads the explorer as not shown (unselected, tooltip "Show explorer"); a press (or the key)
  goes back to the selected tab with the explorer shown, whatever the remembered state was.
  Pros: the button always tells the truth about what is on screen, and a press always gives a
  visible result; same pattern as opening a file from the page (`open_file` leaves the page
  first, decision "Discard prompt Cancel after opening a file from the settings page");
  the Settings row stays, one click back. Cons: a press while the page shows never hides the
  explorer (nothing to hide on screen anyway); needs `window` in the toggle path.
- Option B — Toggle the remembered state only: the page stays, the change shows on leaving
  it. Pros: no navigation. Cons: a key press with no visible effect looks broken; the button
  state would show something not on screen; easy to toggle twice by mistake.
- Option C — Disable the button and ignore the key while the page shows. Pros: no surprise
  navigation. Cons: a dead control and a silent key for no clear reason; needs a disabled
  tooltip to explain it; the sidebar toggle next to it still works, so it is inconsistent.
- Option D — Show the explorer next to the page. Pros: what VS Code does (its Settings editor
  sits beside the side bar). Cons: contradicts the step text (the page hides the explorer);
  the explorer follows the tab's folder, which is unrelated to the page. Would need the user.
- Decision: A, because a visible control should reflect the screen and do something visible,
  and leaving the page is already how file opens behave there. Details: the shown state is
  `explorer_visible && !settings.shows_page()`, used by the button's selected state and label,
  the panel/divider render and `fixed_panes_width`; window state saves the remembered
  `explorer_visible` (the page is not restored at start). Turning "File explorer and viewer"
  off on the page still clears the remembered state (`drop_file_panes`), so the explorer stays
  closed on leaving, as today. Decided by: UX/UI agent

## Viewer "reveal folder" (and other explorer-on paths) while the settings page shows
- Date: 2026-10-26, step: settings-hides-explorer, raised by: Step 0
- Context: `ViewerEvent::RevealFolder` comes only from a folder link clicked in the viewer's
  Markdown page (`file_viewer.rs` `open_link`). The selected tab's viewer is hidden (not
  rendered) while the page shows and other tabs' viewers are hidden too, so it cannot be
  clicked; no other path sets `explorer_visible = true` except `toggle_explorer`; turning the
  file panes on (`PaneChange::Show`) does not show the explorer.
- Option A — No guard: rely on it not firing. Pros: no code. Cons: the handler only calls
  `toggle_explorer` when `!explorer_visible`, so if it ever fires with the explorer
  remembered open it would reveal into a hidden panel and the page would stay.
- Option B — Route it through the same "show explorer" path as the button (Q above, A):
  leave the page if it shows, set the explorer shown, then reveal. Pros: one helper for every
  explorer-on path, correct even if a future path (keyboard link activation, a new command)
  fires it during the page; no extra test burden beyond the button's. Cons: a few lines.
- Decision: B, because it costs nothing over the button's helper and keeps every
  "show the explorer" path consistent with the page; no user-visible change today since the
  event cannot fire while the page shows. Decided by: UX/UI agent
