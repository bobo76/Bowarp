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
  choice that silently doesn't apply or needs a restart. The PLAN.md manual Windows 11 check
  includes switching None/Acrylic/Mica live at 80 %, both ways and through 100 %. If that
  check shows `DWMSBT_NONE` doesn't clear Mica, fall back to A+ with the caption above and a
  known-limitations line. C stays worth posting later (needs the user). Decided by: UX/UI agent
