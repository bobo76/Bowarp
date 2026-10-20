# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (2026-10-19)

[FEATURES.md](FEATURES.md) is the target; what is not built yet is in the milestones and the
backlog below. Details of finished work are in the git history (282 tests).

**Known limitations**

- Not supported in the file viewer: AVIF images (GPUI's decoder lacks it: "The image could
  not be read."), a `#heading` in a document link (the file opens at the top).
- File viewer images: still WebP files share animated GIF/WebP's 2 MB limit (other images
  50 MB), and images over 16384 pixels on a side are refused rather than scaled down.
- An OSC 8 link spanning several lines: only its row is underlined.
- An agent tab going on as a shell starts in the agent's last known folder: on Linux its
  `/proc` folder as of its last title change or hook event, elsewhere the folder it started in.
- F11 has no menu entry: the app has no window menu (a right-click on the title bar opens the
  window manager's own).
- No command history in cmd tabs: cmd's prompt prints only the `D` mark, not the `A` / `B`
  marks that locate the prompt.

**Next steps (quick wins 3)**

1. Settings file and settings page ([reference](docs/ui-reference/settings.png)):
   - `settings.json` in the config folder (next to `tabs.json`), in `workspace-model`: every
     field optional with a default, unknown fields kept, a broken file logged and replaced by
     the defaults (not overwritten). Unit-tested.
   - A settings button in the title bar opens the page; it shows as a "Settings" row in the
     sidebar and takes the terminal area (like a tab, not saved in `tabs.json`, one at most).
   - Built on gpui-component's `setting` module: a search box over item titles and keywords,
     pages on the left, groups of rows (title, description, control) on the right, a reset
     per item, and an "Open settings file" button at the bottom (opens it in the file viewer).
   - Pages for this step: Appearance (font family, font size), Terminal (default shell),
     About (version, config folder). Changes are saved at once and applied live (the
     terminal re-measures its cells and resizes the pty).
   - Later backlog items add their rows here (theme, opacity, shortcuts, launcher paths and
     icons).
2. Font family and size: from the settings file (today hard-coded).
3. Default shell: the new-tab menu's first entry and Ctrl+Shift+T use it; empty keeps the
   $SHELL / Git Bash default.
4. File viewer width: saved in the settings file when it changes and restored at startup
   (not shown on the settings page).

**Fixed, not tested yet (built and unit-tested by the Windows CI, not run on a Windows machine)**

- Command history in PowerShell: Enter on a HISTORY row printed `^E` before the command
  (PSReadLine's Windows edit mode has no Ctrl+E); PowerShell tabs now get End (`ESC [ F`)
  before the Backspaces (one per UTF-16 unit), bash/zsh still Ctrl+E. Check in pwsh and
  Windows PowerShell: type part of a command, Up, Enter on a row: the line holds just that
  command. Also with a grey inline prediction showing after the typed text (no leftover
  text, no bell) and with an emoji on the line.
- The explorer's `cd` into a WSL tab typed the Windows path (`cd 'C:\Users'`, refused by
  bash); WSL tabs now get `ShellSyntax::Wsl`, which types `C:\x` as `/mnt/c/x` (default mount
  root only) and `\\wsl$\<distro>\x` as `/x`. Check in an Ubuntu tab: the parent-folder button
  and "cd to directory" move the shell, and `cd` updates the explorer and the title.

Checked on a desktop (2026-10-19): F11 full screen; on Windows: the parent-folder button in PowerShell, the
explorer following `cd` in PowerShell, Windows PowerShell, Git Bash and cmd (and cmd's title),
Copilot CLI status, no console window from the hover card's `git`, the title bar buttons,
"Bash" opening Git Bash, and Claude Code hooks (`agentws-hook.exe` quoting, `claude.cmd`
under ConPTY).

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
| Title bar | settings button | quick wins 3 |
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
  repeat it (the bell already marks a hidden tab Needs attention).
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
- File viewer: the `⋮` menu from the reference screenshot.
- First-run tab on Windows opens Git Bash: with no saved tabs and no tab on the command line,
  `main.rs` starts `LaunchSpec::shell(cwd)` (no program), which alacritty runs as Windows
  PowerShell, while Ctrl+Shift+T and the `+` menu open `launcher::default_shell` (Git Bash).
  Start the first tab from `default_shell(&launchers)` in the current folder instead, falling
  back to `LaunchSpec::shell` when no shell is found; Unix keeps `$SHELL`.
- Command history with zsh-autosuggestions (likely bug, unverified): Enter on a HISTORY row
  sends Ctrl+E, which the plugin binds to accept the grey suggestion, but the reported line
  (`$BUFFER`, `.zshenv` `__agentws_report_line`) excludes it, so the Backspaces leave the
  accepted rest in front of the new command. Send a key that doesn't accept it, or clear
  `POSTDISPLAY` first.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: the filter/sort button from the reference screenshot.
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
