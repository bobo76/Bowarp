# Agent Terminal Workspace - Desired Features

## Purpose

A fast, lightweight desktop terminal workspace for running multiple independent
Copilot CLI and Claude Code sessions. The application organizes and displays
existing command-line tools; it does not provide its own AI assistant or connect
directly to model APIs.

## Terminal Sessions

- Run multiple interactive terminal sessions simultaneously.
- Launch Copilot CLI, Claude Code, or a regular shell in a selected project folder.
- Support normal interactive CLI behavior, including permission prompts,
  cancellation, multiline input, colors, and terminal resizing.
- Provide copy/paste, text selection, scrollback, and terminal search.
- Shell command history shared by all shell tabs (with folder and time): an inline
  suggestion from history while typing and a history popup above the prompt (Up).
- Support split panes, adjustable pane sizes, and quick switching between sessions.
- Warn before closing a session or workspace with running processes.

## Tabs and Groups

- Organize terminal tabs into named, collapsible groups, such as projects or tasks.
- Collapse a group without interrupting its sessions.
- Create, rename, reorder, and move tabs between groups.
- Assign colors to individual tabs and groups, with tabs optionally inheriting
  their group's color.
- Display an icon identifying the session type, with manual icon overrides.
- Keep session identity, custom colors, and activity status visually distinct.
- Provide keyboard shortcuts for opening, closing, and switching tabs and panes.

## Project Files

- Browse the selected session's folder in a collapsible file tree that follows the
  session's current folder and refreshes when files change.
- Act on files and folders from the tree: create, rename, delete, open a session or `cd`
  there, copy paths, and hand a file's path to the agent as context.
- View and edit a clicked file beside the terminal: Markdown rendered or as source, JSON
  laid out and colored with foldable blocks, other text colored by language; saving with
  Ctrl+S, and reloading when the file changes on disk.

## Agent Activity and Attention

- Indicate when an agent is working, idle, waiting for user input or permission,
  or has exited, when reliable status information is available.
- Show status on individual tabs, including tabs that are not currently selected.
- Show how long an agent has been working on its current turn.
- Show aggregate status on collapsed groups, such as the number of working
  sessions and sessions needing attention.
- Make sessions needing attention easy to locate and select.
- Support completion and attention notifications with user-configurable sound.
- When reliable agent status is unavailable, label terminal output activity as
  "recent activity" rather than claiming the agent is thinking or working.
- Do not treat a running process as proof that the agent is actively working,
  or terminal silence as proof that it is idle.

## Workspace Persistence

- Save named workspaces with their groups, tab names, colors, icons, project
  folders, launch commands, and pane layouts.
- Restore saved organization and offer to relaunch sessions when reopening a
  workspace.
- Clearly distinguish restoring a layout from restoring a live process or an
  agent's conversation.

## Performance and Usability

- Prioritize Windows support.
- Keep typing, scrolling, resizing, and switching sessions responsive while
  multiple agents produce output.
- Minimize startup time, idle CPU usage, and memory overhead.
- Avoid unnecessary background polling and expensive output processing.
- Keep the interface focused on terminals, organization, and session visibility.
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
