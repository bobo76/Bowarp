# Agent Terminal Workspace - Desired Features

## Purpose

A fast, lightweight desktop terminal workspace for running multiple independent
Copilot CLI and Claude Code sessions. The application organizes and displays
existing command-line tools; it does not provide its own AI assistant or connect
directly to model APIs.

## Terminal Sessions

- Run multiple interactive terminal sessions simultaneously.
- Launch Copilot CLI, Claude Code, or a regular shell in a selected project folder,
  from a new-tab menu listing the installed shells (the default shell, chosen in the settings,
  first) and agents, each with its own icon.
- Support normal interactive CLI behavior, including permission prompts,
  cancellation, multiline input, colors, and terminal resizing.
- Provide copy/paste, text selection, scrollback, and terminal search.
- Search terminal output and retained scrollback with Ctrl+Shift+F: case-insensitive literal
  matching, visible match highlights, a current/total count, Enter/Shift+Enter navigation
  with wrapping, and Escape to close.
- Pass mouse wheel, clicks and motion to programs that ask for them (scrolling a Claude Code
  transcript), with a modifier to keep local selection.
- Shell command history shared by all shell tabs (with folder and time): an inline
  suggestion from history while typing and a history popup above the prompt (Up).
- Open URLs and file paths printed in the output with Ctrl+click; a `path:line` opens in
  the file viewer at that line. www.google.ca
- When an agent (Claude Code, Copilot CLI) quits normally (exit code 0), its tab goes on as a
  shell in the agent's folder; when it fails, the tab keeps its last screen to read the error.
- Support split panes, adjustable pane sizes, and quick switching between sessions.
- Run any action or jump to any tab from a command palette.
- Toggle full screen (F11).
- Choose the terminal font family and size, applied live to every tab.
- Light and dark themes, terminal color schemes, and adjustable window opacity.
- Warn before closing a session or workspace with running processes.

## Tabs and Groups

- Organize terminal tabs into named, collapsible groups, such as projects or tasks.
- Collapse a group without interrupting its sessions.
- Create, rename, reorder (menu or drag and drop), duplicate, and move tabs between groups.
- Pin tabs to the top of the list.
- Show each tab's git branch, or its current folder outside a repository, under its name.
- Show a tab's details on hover: status, full folder, branch, program title, session
  kind, and lines added in the working tree.
- Search tabs by name, title, folder, branch, or group, moving through the results with
  Up/Down.
- Copy a tab's branch, title, or working directory; close other tabs or the tabs below.
- Hide and show the sidebar.
- Assign colors to individual tabs and groups, with tabs optionally inheriting
  their group's color.
- Display an icon identifying the session type, with manual icon overrides.
- Keep session identity, custom colors, and activity status visually distinct.
- Provide keyboard shortcuts for opening, closing, and switching tabs and panes.

## Project Files

- Browse the selected session's folder in a collapsible file tree that follows the
  session's current folder and refreshes when files change. The tree's width is resizable.
- Act on files and folders from the tree: create, rename, delete, open a shell or an agent
  session or `cd` there (or to the parent folder), and copy paths.
- View and edit a clicked file beside the terminal: Markdown rendered or as source, JSON
  laid out and colored with foldable blocks, other text colored by language, images (SVG
  drawn or as source; up to 50 MB, animated GIF and WebP up to 2 MB); saving with Ctrl+S, and reloading when the file changes on disk
  (keeping the scroll position); find and replace in the open file. Links between documents
  open in the viewer. The viewer is resizable, and each tab keeps its own open file.

## Agent Activity and Attention

- Indicate when an agent is working, idle, waiting for user input or permission,
  or has exited, when reliable status information is available.
- Show status on individual tabs, including tabs that are not currently selected.
- Show how long an agent has been working on its current turn.
- Show when a command typed at a shell tab's prompt is running, and which command (shell
  integration).
- Show how long the current command has been running in a shell tab.
- Mark an agent that finished while its tab was not selected as "Ready for review" until it
  is viewed; this indicates unread work, not confirmation that the work is correct.
- Mark a tab that rang the bell, or whose long shell command finished (or failed), while
  it was not selected, until it is viewed.
- Read agent status from the CLI's own hooks without changing the user's CLI settings.
- Show aggregate status on collapsed groups, such as the number of working
  sessions, running shell commands and sessions needing attention.
- Make sessions needing attention easy to locate and select.
- Support completion and attention notifications with user-configurable sound.
- When reliable agent status is unavailable, label terminal output activity as
  "recent activity" rather than claiming the agent is thinking or working.
- Do not treat a running process as proof that the agent is actively working,
  or terminal silence as proof that it is idle.

## Workspace Persistence

- Reopen the window with its last size, position, file explorer and viewer widths, and tabs (programs,
  folders, names, colors, pins, groups, order, and selection), starting fresh sessions.
- Save named workspaces with their groups, tab names, colors, icons, project
  folders, launch commands, and pane layouts.
- Restore saved organization and offer to relaunch sessions when reopening a
  workspace.
- Clearly distinguish restoring a layout from restoring a live process or an
  agent's conversation.
- Keep preferences in a settings file that survives hand edits, other running windows and
  unknown or mistyped entries; a file that can't be read is set aside, never lost.
- Edit preferences on a settings page opened from the title bar: searchable pages of
  settings applied as they change, a reset per setting, the file's errors shown, and a
  button to open the settings file itself.

## Performance and Usability

- Prioritize Windows support.
- Keep typing, scrolling, resizing, and switching sessions responsive while
  multiple agents produce output.
- Minimize startup time, idle CPU usage, and memory overhead.
- Avoid unnecessary background polling and expensive output processing.
- Keep the interface focused on terminals, organization, and session visibility.
- Show the version and build in an About dialog.
- Do not require an application account or a separate AI subscription beyond
  whatever the chosen CLI tools require.

## Out of Scope

- Built-in AI chat, model selection, prompt orchestration, or direct model API calls.
- Reimplementing Copilot CLI or Claude Code authentication, permissions, or
  conversation management.
- Recreating all features of Warp or a full IDE.
- Guaranteed survival of terminal processes after the application exits.

## Expected Experience

A user can open a project group, launch several Copilot CLI or Claude Code
sessions, distinguish them by name, color, and icon, and collapse the group
without losing visibility into activity or requests for attention. Reopening
the application restores the workspace organization without falsely implying
that previously running processes are still alive.
