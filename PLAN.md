# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (2026-10-19)

What is done is described in [FEATURES.md](FEATURES.md); the details are in the git history.

**Done: easy wins (one pass), 260 tests**

F11 full screen (window-level `ToggleFullScreen`), 2 lines per wheel notch on Linux, the
explorer's "Open in new tab with" (Claude Code, Copilot) replacing "Attach as context", its
parent-folder button (`cd ..`), the bell marking a hidden tab Needs attention, the exited bar in
the terminal, the file viewer keeping its scroll on reload, and Up/Down in the sidebar search.
Checked under Xvfb (`pipeline/retros/easy-wins/shots/`), except full screen itself: F11 is not
sent to the program and sends the window manager the `_NET_WM_STATE_FULLSCREEN` toggle, but
no window manager runs there. Check on a desktop (X11, Wayland, Windows): F11 enters and leaves
full screen from the terminal, the search field and the explorer, and leaving it returns to the
earlier size or to the maximized state. F11 has no menu entry: the app has no window menu (a
right-click on the title bar opens the window manager's own).
The parent-folder button is disabled while a command runs; at the prompt zsh and PowerShell
change folder through a hidden key (Ctrl+Alt+Shift+F12, folder read from `AGENTWS_CD_FILE`),
bash gets a typed `cd`. Check on Windows that the PowerShell key gets through ConPTY and
PSReadLine: the button moves the shell and the tree, with no stray characters.

**Done: quick wins (one pass), 273 tests**

Ctrl+click links in terminal output (URLs, OSC 8 hyperlinks, existing files as `path`,
`path:line` or `path:line:col`, also across a wrapped line), underlined with a hand cursor
while Ctrl is held; an agent tab going on as a shell when Claude Code or Copilot CLI quits
with code 0 (a failed agent keeps its last screen);
images in the file viewer (raster drawn from the file, SVG drawn or as source, pixel size in
the header); links between documents opening in the viewer; the viewer's divider (drag,
double-click for 45%, minimum 240 px for viewer and terminal, kept as a fraction of the
window); and one viewer per tab. Link detection, `path:line:col` parsing, document links and
the width limits are in `workspace-model` (`links`, `pane_width`). Checked under Xvfb
(`pipeline/retros/quick-wins/shots/`), except the hand cursor (screenshots don't show the
pointer) and the browser itself (a logging `xdg-open` stood in). Not supported: AVIF images
(GPUI's decoder lacks it: "The image could not be read."), a `#heading` in a document link
(the file opens at the top), an OSC 8 link spanning several lines (only its row is
underlined). The shell starts in the agent's last known folder: on Linux its `/proc` folder as
of its last title change or hook event, elsewhere the folder it started in.

**Next steps**

None.

**Fixed, not tested yet (built and unit-tested by the Windows CI, not run on a Windows machine)**

- No command history on Windows (Up at a prompt went to the shell): output is now held after
  the `A` / `B` marks there too, the event loop being woken by a completion packet posted to
  its poller for the pty's key (`Wake` in `term-core/src/prompt.rs`), so the prompt's
  position is tracked. Check in PowerShell,
  Windows PowerShell, Git Bash and an Ubuntu tab: Up opens the HISTORY popup, the dimmed
  suggestion follows typing, Enter on a row replaces the command line (PSReadLine's default
  Windows edit mode may not bind Ctrl+E, which `PromptInput::replace_keys` sends first: try
  it with the cursor moved left), output never stalls after a prompt, and ConPTY's redraws
  don't misplace the popup or the command line read from the screen.

- File explorer not following `cd` on Windows: the shells now report their folder in the `D`
  mark (`cwd=<hex>`), and Windows scans the marks. PowerShell, Windows PowerShell and Git
  Bash (login shell, via `AGENTWS_BASH_LOGIN`) now get the script. Checked outside the app: the scripts' marks, and
  that ConPTY (Windows 11 26100) passes OSC 133 through unchanged. Check in each of those tabs: `cd` updates the explorer, the Running
  indicator works, a new tab opens in the selected tab's folder, and Windows PowerShell
  starts without an execution-policy error.
- File explorer not following `cd` in cmd and WSL tabs, and cmd's title showing
  `cmd.exe`'s path: cmd's `PROMPT` now prints a `D` mark with the folder as text
  (`path=<text>`, cmd cannot print hex) and sets the title to the folder; a WSL distro whose
  login shell is bash starts it with the bash script (`wsl -d <distro> -e sh -c ...`, token
  through `WSLENV`), which reports the folder with `wslpath -w`. The scripts are checked out
  with LF (`.gitattributes`), as WSL's bash rejects CRLF, and arguments are now quoted on
  Windows (`escape_args`). wsl-vpnkit is hidden from the launchers. Checked outside the app:
  the WSL launch's marks and folders, and cmd's prompt. Check in a cmd and an Ubuntu tab:
  `cd` updates the explorer and the title; PowerShell and Git Bash tabs still start.
- Copilot CLI status: the pty reader looks for progress reports (OSC 9;4, which Windows
  Terminal shows as the tab spinner) in every session's output (`ProgressScanner`), and a
  Copilot tab's first one switches it from output activity to agent status: Working (with
  the lap time) while progress shows, Idle or Done once it is removed. Copilot only reports
  progress to terminals it recognizes (from env vars alone, the same JS on every platform),
  so Copilot tabs get `ConEmuANSI=ON` (`session_env`); in Copilot 1.0.98 that type changes
  nothing else. Checked on Linux with Copilot CLI 1.0.98: `9;4;3` when a turn starts,
  repeated every 5 s, `9;4;0` when it ends or is cancelled; the hover card shows Working
  with its time, Idle after the turn, Done in a hidden tab
  (`pipeline/retros/copilot-status/shots/`). A permission prompt keeps `9;4;3`, so it shows
  Working, not NeedsAttention; Esc at the prompt clears it. Check on Windows that the
  progress reports get through ConPTY: Working during a turn in a Copilot tab.
- Console windows flashing over the app: the hover card's `git` child now starts with
  `CREATE_NO_WINDOW`. Check on Windows: hovering tabs in a git folder shows the branch and
  opens no console window.

**Not doable yet**

- Windows: "Bash" opened Ubuntu (Git Bash / WSL distro launchers): code done in `launcher.rs`, needs a Windows machine to build and run (this machine is Linux).
- Windows: check that the title bar buttons (sidebar, explorer, About) click. They were dead
  because gpui-component's `TitleBar` marks its child area `WindowControlArea::Drag`, so
  `WM_NCHITTEST` answered `HTCAPTION` over them and `DefWindowProc`'s move loop ate the
  mouse-up; they now sit in a `div().occlude()` wrapper, which should give them `HTCLIENT`.

**Deferred (needs a Windows machine):** run the CI's Windows build and check ConPTY, the named
pipe and hook command quoting.

## Decisions

- **Stack:** Rust + GPUI + `alacritty_terminal`.
  - Priorities: performance, then memory, then Windows, then Ubuntu.
  - Native GPU rendering, no browser engine, event-driven redraw (zero work when idle).
- **Licensing:** Zed's `terminal` / `terminal_view` crates are GPL-3 - reference only, do not copy.
  `gpui` and `alacritty_terminal` are Apache-2.0.

## Crates (versions as of 2026-10-16)

| Need | Crate | Note |
|---|---|---|
| UI + widgets | `gpui-kit` 0.7.3 | one crate: pins GPUI (`gpui-pre` 0.3.9) and re-exports it with `gpui-component` |
| Terminal emu + pty | `alacritty_terminal` 0.26 | built-in `tty` module (ConPTY on Windows) + `EventLoop` |
| Hook IPC | `interprocess` 2.4 | named pipe (Windows) / Unix socket (Linux) |
| Persistence | `serde`, `serde_json`, `directories` | |
| Sound (M5) | `rodio` | |

## Workspace Layout

```
agent-workspace/
├── crates/
│   ├── term-core/        # session = pty + alacritty Term + event listener (UI-agnostic, testable)
│   ├── agent-status/     # status state machine + IPC server for hook events
│   ├── workspace-model/  # UI-free logic: tab location, layout, persistence, file tree/viewer, history
│   ├── agentws-hook/     # tiny binary invoked by Claude Code hooks
│   └── app/              # GPUI UI: sidebar, terminal element, panes
```

Only `app` depends on GPUI. If GPUI proves too painful, only that crate is replaced (iced/egui).

## Target UI

Reference screenshots: [sidebar + hover card](docs/ui-reference/sidebar-hover-card.png),
[tab context menu](docs/ui-reference/tab-context-menu.png). Still to do from them:

| Element | Look | Milestone |
|---|---|---|
| Title bar | settings button once there are settings | - |
| Context menu | Save as new config (= save as a named workspace) | M4 |
| Clipboard notice | banner above the terminal when a program writes the clipboard (OSC 52), with allow/deny | M5 |

"Share session" from the screenshot is not planned.

## Later Milestones

- **M4:** named workspaces (the single window's tabs are already restored), offer to relaunch; restored tabs clearly marked "not running".
- **M5:** terminal search, close-with-running-process warnings, notifications + configurable sound.
- **M6:** Copilot CLI NeedsAttention (progress reports only give Working / Idle; needs hooks or a
  progress state for prompts), Windows packaging, Ubuntu polish
  (Wayland/X11, socket paths).

## Backlog

- Claude Code typed at a shell prompt behaves like a Claude Code tab: today a `claude` started
  from a shell tab shows "Running for" until it quits, even while it sits idle, because it gets
  no hooks. Instead:
  - Shell tabs also get `AGENTWS_SESSION`, the hook pipe name and a new
    `AGENTWS_CLAUDE_SETTINGS` (the settings file `ClaudeHooks` writes, `crates/app/src/hooks.rs`).
  - The shell integration scripts (bash, zsh, PowerShell) define a `claude` wrapper that runs
    `command claude --settings "$AGENTWS_CLAUDE_SETTINGS" "$@"` when that variable is set.
  - `TerminalView`: while the running command (OSC 133 `C` mark, `PromptEvent::Started`) has
    `claude` as its first word (matched by file name, path and `.exe` / `.cmd` stripped, like
    `shell_icon`), the tab shows the hook status (Working, Waiting, Done, with the "Working ·
    2m 14s" lap) instead of Running, and the sidebar row shows the Claude icon; `copilot` gets
    its icon only (no hooks). The prompt returning (`Finished`) clears the hook status and the
    tab is a shell tab again.
  - Nested shells and tmux clear the session variables, as they already clear
    `AGENTWS_MARK_TOKEN`, so a `claude` started there doesn't report to this tab.
  - Check that a `--settings` the user passes still works alongside the wrapper's (Claude Code
    may take only one; if so, document it).
  Needs shell integration, so it misses `npx`, an absolute path to `claude` and scripts that run
  it (a later Linux `/proc` child check could add the icon for those, without hooks).
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- Bell sound: with the M5 notification sound; rate-limited so a stream of bells doesn't
  repeat it (the attention mark is in the easy wins above).
- Long command finished in a shell tab: when a command that ran longer than a threshold
  (default 10 s, in the settings file) ends while its tab isn't selected, the tab is marked
  done, or failed when the OSC 133 `D` exit code isn't 0, until it is viewed. Needs shell
  integration. Same notification and sound as M5's agent notifications.
- Command palette: Ctrl+Shift+P opens a fuzzy search over every action (the ones with
  shortcuts and the menu-only ones such as Duplicate, Move to group or Copy branch), showing
  each one's shortcut, plus the tabs by name ("go to tab"). Enter runs the action on the
  selected tab.
- Themes: light and dark UI (gpui-component's `ThemeMode`, hard-coded to dark in `main.rs`)
  and terminal color schemes: a few bundled schemes plus user schemes imported from
  base16/iTerm files, saved in the settings file. "System" follows the OS light/dark setting.
- Window opacity and background blur, set in the settings file (opacity applies to the
  terminal background only, text stays opaque); blur where the platform allows it (Windows
  acrylic/mica, KDE on X11).
- File viewer find and replace: Ctrl+F searches the open file (highlight all matches,
  Enter / Shift+Enter to move, match count, case and whole-word toggles), Ctrl+H adds a
  replace field (replace one / all) in source mode; rendered Markdown only gets find.
- Configurable font family and size.
- Configurable default shell for the new-tab menu's first entry and Ctrl+Shift+T, saved in the settings
  file (overrides the $SHELL / Git Bash default).
- Save the file viewer's width (resizable since the quick wins) in the settings file and
  restore it at startup.
- File viewer: the `⋮` menu from the reference screenshot.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: the filter/sort button from the reference screenshot.
- Configuration panel: a settings dialog opened from the title bar's settings button (see the
  title bar row above) that edits the settings file, for the configurable items in this list.
- Configurable terminal path: the executable path for each launcher entry (Terminal, Git Bash,
  `claude`, `copilot`) saved in the settings file, for programs that aren't on `PATH` or
  where the wrong copy is found first; an empty value keeps the current lookup.
- Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
  `cx.build_action`) in place of `default_bindings`.
- Configurable terminal icon: the icon for each launcher entry, picked from the bundled
  `LauncherIcon` set (Bash, PowerShell, Command Prompt, Zsh, Fish, Ubuntu, ...) and saved in
  the settings file; it shows in the new-tab menu and on the sidebar row of every tab started
  from that entry (restored tabs included). "Automatic" keeps the current detection
  (`shell_icon` / `distro_icon` in `workspace-model/src/launcher_icon.rs`), e.g. for a
  wrapper script that `shell_icon` only sees as `Shell`.
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
- Group hover card: hovering a group header for a moment opens a card next to it, like the
  tab hover card (`with_hover_card` in `app/src/workspace_view/sidebar_rows.rs`), that sums up
  the group's tabs. Most useful when the group is collapsed:
  - Counts: "5 tabs · 2 Working · 1 Waiting · 1 Done", then one line per tab: kind icon,
    name, status with its lap time ("Working · 2m 14s", "Running for 40s") and branch, in
    sidebar order; tabs that need attention (Waiting, unviewed Done or failed) first.
  - Common folder or branch shown once in the header when every tab shares it.
  - Clicking a line selects that tab (expanding the group). The card ticks the lap times while
    it shows (`set_card_clock` for each working tab) and is built only while it shows; no
    card during a drag or over an empty group.
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
- Draw box-drawing and block characters procedurally; font glyphs leave small gaps between rows for heavy/double verticals (`┃`, `║`).
- Answer OSC 52 clipboard requests.
- Report drags that leave the terminal area (GPUI only delivers moves while hovered).
- Working directory from OSC 7 (only useful for remote shells): `alacritty_terminal` does not
  parse it, but the pty reader wrapper (`term-core/src/prompt.rs`) could find it.
- Untested on Windows: hook command quoting (`"C:\...\agentws-hook.exe"` run by Claude Code's
  shell) and `.cmd` launchers (`claude.cmd` from npm) under ConPTY.
- Cache the active TerminalView (`.cached(...)`) so sidebar animation frames and other
  WorkspaceView re-renders don't re-render the terminal; every input the view shows (focus,
  resize, selection, session wakeup) must notify it; check with Xvfb.
- PowerShell: the integration writes the `C` mark on Enter even when PSReadLine only adds a
  line to an unfinished command (`if ($x) {`), so the tab shows Running while typing; write it
  only for accepted lines (needs a machine with pwsh to check).

## Maybe

Ideas not committed to yet; not in [FEATURES.md](FEATURES.md) until picked.

- Diff panel: the selected tab's working-tree changes (`git diff` against `HEAD`, untracked
  files included) in the file viewer pane, one collapsible section per file with added and
  removed lines colored, opened from the hover card's `+N` or a title bar button, and
  refreshed when the file watcher sees a change. Shows what an agent changed without leaving
  the app; read-only (no staging or reverting).
- Git worktree and pull request on the sidebar row: a worktree marker when the tab's folder is
  a linked worktree (`.git` is a file), with the main repository in the hover card; the open
  PR's number and state for the branch, from `gh pr view --json` run off the UI thread and
  cached per branch, skipped when `gh` is missing or not logged in.
- Sticky command header: while scrolling back through a shell tab's output, the command that
  produced the top visible line stays pinned above the terminal (from the OSC 133 marks);
  clicking it scrolls to that command's prompt. Needs shell integration; not for agent tabs.
- Secret redaction: text matching secret patterns (API keys such as `sk-...`, `ghp_...`,
  AWS keys, `password=...`, plus user regexes from the settings file) is drawn masked in the
  terminal and copied masked unless the copy is confirmed; hover or a toggle reveals it. Agents
  often print `.env` contents. Display only, the program's output is unchanged.

## Risks

- **GPUI API churn:** exact `gpui-pre` version; UI isolated in `app`.
- **IME and clipboard edge cases** in a custom terminal element: test early in M1, especially on Windows.
- **Claude Code `--settings` flag and hook payload shapes:** verify against current docs before relying on them.
