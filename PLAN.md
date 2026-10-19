# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (2026-10-18)

What is done is described in [FEATURES.md](FEATURES.md); the details are in the git history.

**Next steps, in priority order**

(none)

**Fixed, not built yet (the Windows machine has no MSVC Build Tools)**

- File explorer not following `cd` on Windows: the shells now report their folder in the `D`
  mark (`cwd=<hex>`), and Windows scans the marks without holding output (no prompt
  position, so no history yet). PowerShell, Windows PowerShell and Git Bash (login shell, via
  `AGENTWS_BASH_LOGIN`) now get the script. Checked outside the app: the scripts' marks, and
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
  the lap time) while progress shows, Idle or Done once it is removed. Not checked against
  Copilot's real output: assumed to show `3` (busy) while working and `0` (remove) when the
  turn ends; any shown state counts as Working, so a permission prompt is not
  NeedsAttention. Check in a Copilot tab: Working and its time during a turn, Idle after it,
  Done when it ends in a hidden tab, and whether a permission prompt or Esc clears it.

**Not doable yet**

- Windows: "Bash" opened Ubuntu (Git Bash / WSL distro launchers): code done in `launcher.rs`, needs a Windows machine to build and run (this machine is Linux).
- Windows: check that the title bar buttons (sidebar, explorer, About) click. They were dead
  because gpui-component's `TitleBar` marks its child area `WindowControlArea::Drag`, so
  `WM_NCHITTEST` answered `HTCAPTION` over them and `DefWindowProc`'s move loop ate the
  mouse-up; they now sit in a `div().occlude()` wrapper, which should give them `HTCLIENT`.

**Deferred (needs a Windows machine):** Windows build and the same check (ConPTY, named pipe,
hook command quoting).

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

- Windows: command history. It needs the prompt's position (`A` / `B` marks held until the
  parser reaches them, `MarkFilter`), whose Unix wake-up socket has no stand-in for the
  Windows pty reader yet (`HOLDS_PROMPTS` in `term-core/src/prompt.rs`).

- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- Show an "exited" banner in the terminal area (currently only in the title).
- Tune wheel scroll speed (about 2 lines per notch on X11).
- Quitting an agent falls back to a shell: when a Claude Code / Copilot tab's program exits,
  the tab starts the default shell in the agent's last folder (its last known `location()`,
  since `current_dir` is `None` once the child exited) and becomes a plain shell tab: shell
  icon and status, saved as a shell in `tabs.json`. Today the tab is left dead as "Exited".
  The new terminal starts empty, so the agent's output is lost, the
  `claude --resume <id>` line included (decision in [README.md](README.md#design-decisions)).
- Agent icon in a shell tab: while `claude` or `copilot` runs from a shell tab's prompt, its
  sidebar row shows the Claude / Copilot icon, and the shell icon returns at the next prompt.
  The command comes from the shell integration's OSC 133 `C` mark (`PromptEvent::Started` in
  `term-core/src/prompt.rs`, which the Running status already uses); its first word is matched
  by file name (path and `.exe` / `.cmd` stripped, like `shell_icon`). Needs shell
  integration, so it misses scripts, `npx` and aliases. Icon only: the status stays shell-like
  (no `--settings` hooks for a `claude` typed at the prompt).
- Clickable links in terminal output: Ctrl+click opens a URL in the browser, both plain-text
  URLs found in the visible grid and OSC 8 hyperlinks (`alacritty_terminal` keeps them on the
  cell). A `path`, `path:line` or `path:line:col` (`src/foo.rs:42`, which Claude Code prints
  all the time), resolved against the tab's folder and only if it exists, opens in the file
  viewer scrolled to that line. Underline and hand cursor while Ctrl is held over a link.
- Bell as an attention signal: a BEL (`SessionEvent::Bell`, ignored today in
  `terminal_view.rs`) in a tab that isn't selected marks it as needing attention until it
  is viewed, like a finished agent. This covers programs without hooks (Copilot, scripts).
  Optional sound with the M5 notification sound; rate-limited so a stream of bells doesn't
  repeat it.
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
- File explorer "cd ..": a parent-folder button in the explorer header (Lucide `folder-up`
  icon, a folder with an up arrow, added to `crates/app/assets/icons/`; tooltip "Parent folder
  (cd ..)") and/or a `..` row above the root. Both emit `ExplorerEvent::CdTo` with the parent
  folder, so a shell tab types `cd <parent>` and the tree follows the new working directory.
  Disabled at a filesystem or drive root, and in a tab that isn't a shell (`can_cd` false:
  Claude Code, Copilot), since the tree always shows the session's folder.
- File explorer entry menu: remove "Attach as context" (`EntryCommand::AttachContext`,
  `ExplorerEvent::Attach`, and `file_tree.rs`'s prompt text if nothing else uses it), and add
  "Open in new tab with" below "Open in new tab" with a submenu of Claude Code and Copilot,
  started in the entry's folder (the folder itself, or a file's parent folder) like the
  new-tab menu's agent entries. "Open in new tab" keeps opening the default shell. Update the
  README's explorer menu description.
- Resizable file viewer pane: drag the divider on the viewer's left edge to resize it
  (currently fixed at 45% of the width, `VIEWER_WIDTH` in `app/src/workspace_view.rs`); resize
  cursor on hover, a minimum width for both the viewer and the terminal, double-click resets
  to 45%. The width is kept as a fraction so it follows window resizes, and is saved in the
  settings file and restored at startup.
- File viewer per tab: each tab remembers its own open file, or none (today one `FileViewer`
  in `WorkspaceView` is shared by all tabs). Selecting a tab shows the file it had open, or
  hides the pane if it had none: open a file in tab 1, select tab 2 and the pane hides, select
  tab 1 again and it comes back with the same file; if tab 2 has another file open, switching
  between the two tabs switches the file shown. Closing the viewer only closes it in the
  selected tab, and closing a tab drops its file.
- File viewer: keep the scroll position when the file is reloaded after a change on disk
  (it goes back to the top); open links to other files (`FEATURES.md`) in the viewer instead
  of the system handler; the `⋮` menu from the reference screenshot.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: Up/Down to move through the matches from the search field; the filter/sort
  button from the reference screenshot.
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
