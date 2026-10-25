# Changelog

User-visible changes are listed by release. Maintenance work and internal refactors are
omitted unless they affect how the app is used.

## Unreleased

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
