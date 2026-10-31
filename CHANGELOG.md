# Changelog

User-visible changes are listed by release. Maintenance work and internal refactors are
omitted unless they affect how the app is used.

## Unreleased

## 0.2.52

- Rename the selected tab with F2 while the sidebar tab list has the keyboard; the terminal
  still gets F2. The key is "Rename tab" on the Keyboard settings page and shows next to
  "Rename tab" in the tab menu. When the rename ends the keyboard goes back to the list.
- Enter in a tab or group name editor no longer also ends a tab search and moves the keyboard
  to the terminal.

## 0.2.51

- Move the keyboard into the sidebar tab list: Tab (or Down with an empty query) in "Search
  tabs..." enters it, Up / Down / Home / End select tabs, Enter, Escape or Tab return to the
  terminal and clear the search, Shift+Tab goes back to the field. A focus ring marks the row.

## 0.2.50

- Add "Reset name" to the tab menu of a renamed tab: the tab shows the program's title again
  (the agent's own name for Claude Code and Copilot tabs).

## 0.2.45

- Show the turning working spinner on renamed Claude Code tabs, whose custom name hides
  Claude Code's own animated title.
- When an agent's status hooks can't be installed for a tab, a warning now appears in the app
  instead of only on the console (which Windows release builds don't have).
- Agent status keeps arriving from sessions started by an older running instance after the
  app is upgraded in place.

## 0.2.43

- Keep agent launch arguments and font-family edits when leaving their Settings fields.
- Clarify that clipboard Ask / Allow / Deny controls terminal OSC 52 writes, not native
  clipboard access by programs such as Claude Code and Copilot CLI.
- Known Copilot Autopilot sessions ignore automatically approved permission hooks instead of
  showing false Waiting notices; real questions still need attention. Launch mode is available
  before the optional extension's first report.
- Copilot launch mode follows flag precedence: the last `--mode` wins, while
  `--plan --mode autopilot` starts in Plan and keeps permission attention enabled.
- Window opacity now also applies to the sidebar and file explorer, including selected and
  hovered rows, while the file viewer, Settings, title bar and overlays stay opaque.
- Extend the Windows Mica backdrop into the client area and reset the frame extension when
  leaving Mica. A failed DWM call now shows a persistent warning instead of failing silently.

## 0.2.42

- A softer light theme: warm grey surfaces instead of white, darker muted text, visible
  borders, and status colors that stay readable on the light sidebar.
- A Sepia theme: paper-toned surfaces with brown text and accents.
- Settings > Appearance picks the theme from a Theme dropdown (Dark / Light / Sepia) instead
  of a Light theme switch.
- The terminal follows the theme: Light and Sepia get light terminal colors. A Terminal colors
  setting keeps a fixed scheme instead (e.g. a dark terminal with the Light theme); changing it
  repaints open tabs at once.

## 0.2.41

- Keep command history per shell: each shell keeps its own newest 2 000 commands, and the
  inline suggestion comes from the tab's own shell only, so a PowerShell tab no longer
  suggests bash commands.
- The HISTORY popup opens on the tab's shell, with a chip per shell plus "All"; switch with
  Alt + the chip's underlined letter (press again for the next chip with that letter) or a
  click. Delete forgets a command in the selected shell only, or in every shell under "All".
- Commands saved before this version have no shell: they show under "All" only and are no
  longer suggested inline.

## 0.2.40

- Copilot CLI tabs show Waiting again while a permission prompt is open, and now also while
  Copilot asks a question ("Waiting for your reply"), with the toast and Ctrl+Shift+A; the
  status stays Waiting until the prompt is answered.

## 0.2.39

- Dim the entries git ignores in the file explorer (`target/`, `.git`, `*.log`...), keeping
  the folder order; folders outside a repository show as before. Saving a `.gitignore`
  updates the dimming of the open folders.
- The working Copilot tab's spinner turns once a second (was every two seconds); several
  working Copilot tabs now turn together on one timer instead of one each.

## 0.2.38

- Keep the window open when the last tab closes: it shows "No open tabs" with a button per
  shell and agent (the default shell first, focused), and the sidebar keeps every group.
  Ctrl+Shift+T opens the new tab in the last closed tab's folder.
- Keep the groups (names, colors, order) at the next start when no tab was open on quit; the
  default shell opens as the first tab.
- With no tab open, tab shortcuts (Ctrl+Shift+W, Ctrl+Tab, Ctrl+1..9) do nothing, the file
  explorer is hidden until the next tab, and "Open settings file" is disabled.

## 0.2.37

- Say when agent status is off at start (for example "Agent status is off: agentws-hook not
  found next to the app. Reinstall to fix."), in a warning that stays until closed. Quiet
  Claude Code tabs then read "Status unavailable" instead of "No recent output".

## 0.2.36

- Open new tabs (the `+` menu, Ctrl+Shift+T, the explorer's "Open in new tab") right below
  the selected tab, in its group, instead of at the end of the group or list. With a pinned
  tab selected, a new or duplicated tab goes first after the pinned tabs.

## 0.2.35

- Hide the file explorer while the settings page shows; it comes back as it was (width,
  expanded folders) when the page closes. The explorer button and Ctrl+| on the page leave
  the page and show the explorer.

## 0.2.34

- Attention toasts say what the agent waits for: "Waiting for permission" or "Waiting for
  your reply" (a bell still shows "Needs attention"); hovering the toast shows the key that
  goes to the next tab needing attention.

## 0.2.33

- Don't show tab and group hover cards while a name is being edited, or a menu or dialog is
  open; a card hidden that way shows again once the pointer leaves the row and comes back.
- After dropping a dragged tab, the card under the pointer no longer pops up at once.

## 0.2.32

- Hide the file viewer while the settings page shows, so the page gets the full width; the
  viewer keeps its file and unsaved edits and comes back with the tab.
- **Open settings file** and opening a file from the explorer while the settings page shows
  now go back to the selected tab and show the file there, instead of beside the page.

## 0.2.31

- Add **Clipboard writes from programs** (Ask, Allow, Deny) to Settings → Terminal → Clipboard,
  also `clipboard_writes` in the settings file: Allow or Deny answers every program's clipboard
  write in every tab without showing the notice.
- Fix the Linux release build not starting on Ubuntu 24.04 (it needed glibc 2.43).

## 0.2.30

- Show a blue ring turning in quarter steps left of a working Copilot tab's title in the
  sidebar; it stops when Copilot is no longer working. With **Activity animations** off it
  stays still. It doesn't animate in a collapsed group or a minimized window.
- Stop the Running badge's pulse while the window is minimized.

## 0.2.29

- Add **Opacity** (30-100 %) and **Background blur** to Settings → Appearance → Window, also
  `window_opacity` and `window_blur` in the settings file. Opacity applies live to the
  terminal's default background only; text, colored cells and the rest of the window stay
  opaque.
- Blur offers None / Acrylic / Mica on Windows (Mica uses Acrylic before Windows 11 22H2) and
  None / Blur on KDE Plasma (Wayland). Elsewhere, or at 100 % opacity, the choice is disabled
  and says why.
- Save a slider value (such as the font size) when no settings file exists yet; it was lost.

## 0.2.28

- Add a **Keyboard** settings page to edit shortcuts: click a key to record a new one, **+** to
  add a key, **×** to remove one, **Reset to default** per action and **Reset all**. A key
  taken from another action moves to the new one, and a notice says so.
- Refuse keys that belong to the terminal program while recording: typing keys and Ctrl or
  Alt plus a letter alone (Ctrl+D, Ctrl+C, Alt+B). Esc cancels recording.
- Show the keys in use in menu and tooltip hints, and no key while an action is unbound. The
  explorer hint now reads Ctrl+|, the key that is actually bound.

## 0.2.27

- Rebind, add or remove keyboard shortcuts with a `"keybindings"` section in the settings file,
  e.g. `{"ctrl-v": null, "ctrl-alt-t": "NewTab"}`. A removed shortcut's key goes to the
  program, so `"ctrl-v": null` gives bash and vim their Ctrl+V back. Changes apply when the
  settings are read again (opening Settings or saving the file in the viewer).
- List each skipped shortcut entry (unknown action, key that can't be read) on the settings
  page; the other entries still apply.
- Allow rebinding F11 (full screen) like the other shortcuts.

## 0.2.26

- Ask before a program writes the clipboard or the primary selection (OSC 52, e.g. Copilot
  CLI's copy on Windows, tmux, vim): a notice above the terminal shows the text's size and a
  preview, and **Allow** (Ctrl+Shift+Y), **Deny** (Ctrl+Shift+N) or **Always for this tab**
  answers it. A newer write replaces the waiting one, and a background tab with a waiting write
  shows Needs attention.
- Show both texts in the notice when a program writes different text to the clipboard and
  the primary selection.

## 0.2.25

- Ask before closing a tab whose command is still running or whose agent is working or
  waiting for an answer. Closing several tabs (a group, other tabs, tabs below) or the window
  asks once and lists every busy tab; idle prompts and idle agents close without asking.
- Refuse an unknown session kind on the command line (such as a typo, or a folder given
  without `shell`/`claude`/`copilot`) with a usage message instead of opening a shell.

## 0.2.24

- Add an **In-app notifications** switch (Settings → Notifications). Off hides agent
  status toasts (Needs attention, Ready for review, clean exits); status and attention
  badges stay, and a crashed agent, user-action feedback and persistent warnings still show.

## 0.2.23

- Add a **File explorer and viewer** switch (Settings → Workspace). Off resolves unsaved
  edits first (Discard or Cancel), then hides both panes and their shortcuts and stops
  watching folders; terminal file links ask to open externally. Pane widths stay saved.

## 0.2.22

- Add a Workspace settings page with **Restore previous tabs** and **Remember last shell
  command** switches. Turning restoring off takes effect at the next start and leaves the
  saved tab snapshot untouched; clearing saved session commands also removes them from that
  retained snapshot. Remembering the last command needs both Command history and restoring
  on, and is shown disabled otherwise.

## 0.2.21

- Add **Command history**, **History popup** and **Inline suggestions** switches
  (Settings → Terminal); the last two are disabled while command history is off. Existing
  history is kept, not erased, while a feature is off.
- Add **Clear history** and **Clear saved session commands** buttons that act after a confirmation;
  a failed clear shows an error.

## 0.2.20

- Add **Group hover cards** and **Activity animations** switches (Settings → Appearance).
  Controls that depend on another switch now show as disabled with the reason.

## 0.2.19

- Keep workspace shortcuts from also reaching the terminal, including preventing
  Ctrl+Shift+D from sending EOF to the tab being duplicated.
- Save last-command snapshots in the background without a debounce delay; normal close
  waits for the latest state. A crash can still lose queued or in-flight updates.
- Clean up Copilot permission plugins when sessions close, exit, are replaced or fail
  to launch, and remove stale plugins from exited instances at startup.
- Explain Windows path quoting in agent argument settings.

## 0.2.18

- Label unread background agent completions "Ready for review" in the sidebar, tab hover
  cards, group summaries and existing completion toast. Selecting the tab restores Idle;
  new Working or Needs attention reports replace it. This does not certify the work's correctness.

## 0.2.17

- Offer a restored bash/zsh tab's last useful command as a one-shot suggestion, accepted
  with Right or End, never run automatically.
- Configure tab hover cards and individual details on the Appearance settings page.
- Mark Copilot CLI tabs Waiting on permission requests through isolated per-tab plugins
  (CLI 1.0.103); Working/Idle still use terminal progress reports.
- Configure extra Claude Code and Copilot CLI launch arguments on the Terminal settings
  page; restored and duplicated tabs keep their original arguments.

## 0.2.16

- Close a focused Settings page from the gear button, or focus it first from another pane.
- Add a file viewer overflow menu for Save and Raw/Rendered, keeping the existing controls.
- Show a persistent warning with the failure reason when startup uses a fallback shell.
- Show tab counts and Working/Waiting/Done totals when hovering a group, including empty groups.
- Show silent in-app toasts when background agents need attention, finish a turn, or exit;
  click a toast to select its tab.
- Persist a light/dark UI choice and apply it live without changing terminal colors.

## 0.2.15

- Show Copilot CLI's interaction mode in the tab hover card through an optional extension.
- Search terminal output and scrollback with Ctrl+Shift+F.

## 0.2.12

- Remember the file viewer width between app launches.

## 0.2.11

- Choose the default shell on the Terminal settings page. It appears first in the new-tab menu
  and is used by the new-shell shortcut.

## 0.2.10

- Load the terminal font family and size from the settings file.

## 0.2.9

- Add an in-app Settings page for browsing and changing preferences.

## 0.2.8

- Load and save persistent application settings.

## 0.2.7

- Support larger images in the file viewer, with a separate 50 MB limit for supported formats.

## 0.2.6

- Improve command-history replacement in PowerShell and path handling in WSL tabs.
- Add Windows and Linux installation scripts.

## 0.2.5

No release tag was published for this version; its changes are included in 0.2.6.

## 0.2.4

- Open terminal links, including file paths, with Ctrl+click.
- Give each tab its own file viewer state.

## 0.2.3

- Add full-screen mode (F11) and parent-folder navigation in the file explorer.
- Show shell command activity and Copilot CLI status in tabs.
- Mark tabs needing attention when they ring the terminal bell.
- Improve shell prompt and working-directory tracking, including on Windows.

## 0.2.2

- Show how long a running command has been active in the tab hover card.

## 0.2.1

- Show a running-command indicator in shell tabs.

## 0.2.0

- Mark the M2 milestone, including the file explorer, file viewer, and shell command history.
- Add an About dialog with the app version, build commit, and repository link.
