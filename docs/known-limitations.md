# Known limitations

How built features behave at their edges, decided on purpose or accepted for now. Work still
to do is in [BACKLOG.md](../BACKLOG.md); the target spec is [FEATURES.md](../FEATURES.md). Remove an
entry when the limitation is lifted.

- Busy-close warnings know only what the shell integration marks and agent status report:
  a tab counts as busy while a command started at its prompt runs (OSC 133 `C` without `D`)
  or its agent is Working or Waiting. Never busy: cmd tabs (no `C` mark), shells without the
  integration's marks (a shell started inside a tab, ssh), jobs left running in the
  background (`&`), Copilot before its first progress report and agents without status
  hooks. Closing the window still drops unsaved file viewer edits without asking.
- In-app notifications off drops only the agent status toasts (Needs attention / Waiting for
  permission / Waiting for your reply, Ready for review, Agent finished on a clean exit); a
  toast already shown stays until it hides. A background agent that exits with a nonzero or
  unknown code still toasts, as do user-action feedback (save, delete, clear history, start
  failures) and the persistent fallback-shell and agent-status-off warnings. Agent toasts are
  keyed per tab, so a new one replaces the tab's earlier one.
- Agent status off (`agentws-hook` missing or the status listener could not start): Claude
  Code tabs fall back to output activity ("Status unavailable" when quiet) and a bell asks for
  attention as in a shell; a Copilot CLI permission prompt reads Working, not Needs attention,
  unless it rings the bell. The warning shows once per start, not again when an agent tab opens.
- Copilot CLI Waiting ends on its next tool hook or when the turn ends: after a question is
  answered, a reply with no tool call keeps the tab Waiting until the turn finishes.
- Copilot Autopilot permission hooks do not show Waiting when the mode is known. The optional
  extension reports live mode changes; without it, only the launch mode (`--mode`,
  `--plan` or `--autopilot`) is known. `--plan --mode autopilot` starts in Plan, so permission
  hooks still ask for attention until the extension reports Autopilot.
  Switching modes inside the CLI without the extension can therefore
  leave the app's mode stale. `--yolo` grants permissions; it does not select Autopilot.
- Clipboard protection covers terminal OSC 52 writes only. Native system clipboard writes
  by programs such as Claude Code and Copilot CLI (especially on Windows) bypass the terminal:
  Ask cannot hold them and Deny cannot block them. One answer covers held OSC 52 clipboard
  and primary-selection writes; a write held when the program exits is dropped.
  "Always for this tab" survives an agent tab going on as a shell. No toast: a background tab
  only shows Needs attention.
- Not supported in the file viewer: AVIF images (GPUI's decoder lacks it: "The image could
  not be read."), a `#heading` in a document link (the file opens at the top).
- File viewer images: still WebP files share animated GIF/WebP's 2 MB limit (other images
  50 MB), and images over 16384 pixels on a side are refused rather than scaled down.
- An OSC 8 link spanning several lines: only its row is underlined.
- With no tab open (the empty state), the file explorer is hidden and can't be used to pick a
  folder first; the next tab starts in the last closed tab's folder, which is kept in memory
  only: after a restart with no tab saved, the first tab starts in the app's working folder.
- An agent running in place in a shell tab: WSL tabs get the hook variables but don't pass them
  into the distro (no `WSLENV` entry), and cmd tabs can't print the mark. The agent's title is
  put back to the shell's when the `D` mark is read, so a title the agent sets in its very last
  output, read together with the mark, can stay until the shell sets one. With the mark printed
  by hand (no end mark), one command line running two agents switches to the second at its mark
  without an exit toast for the first, and a command after the agent on the same line (`claude;
  make`) runs under the agent's icon and status, without a Running badge or a close question,
  until the line's `D` mark. The same holds when the agent is killed by a signal (bash and zsh
  then abort the rest of the line, end mark included). An agent suspended with Ctrl+Z ends its
  run (bash goes on with the line); `fg` resumes it as a plain shell command, without the
  agent's status.
- Typed `claude` / `copilot` (bash and zsh): only an interactive run at the prompt gets the
  hooks. Not wrapped: a user function of the same name, or a user alias that doesn't start with
  the agent's name (they win: put the hooks in them by hand; a function defined later at the
  prompt, e.g. by re-sourcing `~/.zshrc`, needs `unalias claude` first: zsh refuses it),
  `sudo claude`, `env claude`, `xargs claude` and scripts (only the typed word is an alias),
  and an agent started from another program. A Claude run with its own `--settings` (typed or
  in an alias) runs unwrapped, without the status; Copilot's `--plugin-dir` can be repeated, so
  it is wrapped. Which runs are one-shot is decided from
  the arguments: a subcommand first, or `-p`/`--print`/`--prompt`, `-v`/`--version`,
  `-h`/`--help` anywhere before `--` (an option value equal to one of these counts too); a
  subcommand added by a later agent version runs in place until the list is updated.
- Typed `claude` / `copilot` (PowerShell): the same rules, with these differences. The
  functions apply wherever PowerShell runs them, scripts run in the tab included. Output
  redirected to a file or assigned to a variable (`claude > out.txt`, `$x = claude`) isn't
  detected: such a run gets the agent mark and status until it exits. After Ctrl+C stops the
  command line, no end mark is printed: the prompt's `D` mark ends the run. PowerShell's `D`
  mark reports 0 on success, else the last program's exit code, else 1; after a failed cmdlet
  it repeats the code of a program that failed earlier (`$LASTEXITCODE` isn't reset), and a
  function (the agent functions included) counts as a success whatever its programs returned.
  `--` may be removed by PowerShell before the function sees it. A function of your own that
  starts the agent by hand (see the README) has the status until the line's `D` mark, and the
  program's exit code isn't reported: the line counts as a success.
- An agent tab going on as a shell starts in the agent's last known folder: on Linux its
  `/proc` folder as of its last title change or hook event, elsewhere the folder it started in.
- Window opacity: on X11 without a compositor the window can't be see-through, and the
  terminal's background probably looks darker instead (seen under Xvfb, not checked on real
  hardware). With the Linux title bar's window shadow, the shadow
  darkens the see-through background by about a third, most visible at low opacity (at 30 %
  the window looks about 53 % opaque); not on a maximized window (from GPUI's shadow shader;
  not checked on real hardware). On Windows before 10 1809 (build
  17763) GPUI applies neither transparency nor acrylic. Linux blur is offered only on KDE Plasma's
  Wayland session (`XDG_CURRENT_DESKTOP` naming KDE): another compositor with KDE's blur
  protocol is shown as unavailable. A program that paints its whole screen with its own
  background color (some vim themes) stays opaque.
- F11 has no menu entry: the app has no window menu (a right-click on the title bar opens the
  window manager's own).
- No command history in cmd tabs: cmd's prompt prints only the `D` mark, not the `A` / `B`
  marks that locate the prompt.
- Command history off keeps stored history and each tab's saved `last_command` (still
  written to `tabs.json` unchanged) but uses neither; "Clear history" truncates
  `history.jsonl` on the UI thread (like deleting one command), and a second instance only
  sees either clear after its next restart (history is loaded at start). Clearing saved
  session commands reaches only tabs open in this instance.
- Command history per shell: commands saved before it have no shell and show only under the
  HISTORY popup's "All" chip (never as an inline suggestion). Alt+letter switches chips by
  the Latin letter the key reports: on non-Latin layouts (Cyrillic, Greek...) it may not
  switch; click the chip instead. While the popup shows chips, Alt+ a chip's letter doesn't
  reach the shell (readline's Alt+B / Alt+F when a chip starts with B or F). A tab's shell is
  fixed when its session starts, from the launcher with the same program and arguments, else
  from the program's name (`wsl -d X`, Git for Windows' bash): a shell started through a
  wrapper script records under the script's name. With "Restore previous tabs" off,
  deleting a command still removes it from every saved tab in `tabs.json`, whatever its
  shell (the saved tabs don't record one). Starting an older version rewrites a long history
  file without shells (all of it then shows only under "All") and keeps only its newest 2 000
  commands.
- Last-command saves go through a background writer: a crash can lose queued or in-flight
  updates, including a command just deleted from HISTORY.
- Tab names, pins, order, groups and colors reach `tabs.json` only at window close or a
  shell's next last-command change: a crash loses those made since.
- Restore previous tabs off: `tabs.json` is kept unread and unwritten (the one exception:
  "Clear saved session commands" and forgetting a command remove just the matching
  `last_command` values from it, on the tabs writer thread), so the next
  save after turning it back on replaces the old snapshot; loading is decided once at start
  (turning it on mid-session restores nothing until the next start). Window bounds are not
  part of the snapshot and are still saved. A second instance's restore decision uses its own
  settings read at its start. "Remember last shell command" off still writes retained
  `last_command` values back unchanged while restoration is on: a command run meanwhile is
  not recorded, and the older value returns when it is switched on again (as with Command
  history off).
- Settings file: a field with a value of the wrong type gets its default silently and the
  value is dropped at the next change. A file that isn't a JSON object is ignored (reason
  printed and kept for the settings page) and moved to `settings.json.bak` at the first
  change. Unknown fields are kept but rewritten in alphabetical order; JSON comments aren't
  allowed. Edits made on disk while running apply when the settings page opens, when the
  file is saved in the file viewer, or at the next change from the app; edits made in another
  editor while the page shows apply only then (no file watch).
- Activity animations: the switch gates the Running badge's pulse and the working
  spinner (Copilot, or Claude Code with a custom name). Under software rendering both are
  already still. A window covered by others keeps animating on Windows and under
  compositing X11 window managers (GPUI reports it visible there); minimized stops it.
- Terminal font: a family that isn't installed (exact name; any case on Windows) silently uses
  the built-in one (DejaVu Sans Mono, Cascadia Mono on Windows); the page doesn't say so. So do
  GPUI's `.`-names (`.SystemUIFont`) and a localized or secondary family name (Linux lists only
  each font's first family name), even when the font would load under it. A font
  change shows in a tab's terminal when it is next drawn, so background tabs (and their pty
  size) catch up when selected. The slider covers 8-32 points; the file takes 6-72.
- Default shell (Terminal page): only the shells found at startup are offered (one installed
  while the app runs shows up at the next start). The choice is saved by its launcher id
  (`zsh`, `git-bash`, `wsl:Ubuntu`); the file also takes a menu name (`Zsh`). One that isn't
  found shows as "(not found)" and the automatic shell opens instead. A change applies to new
  tabs only: open and restored tabs keep the shell they started with. On Unix, when `$SHELL`
  isn't listed, the first tab runs it but Ctrl+Shift+T opens the first listed shell (the
  page then says just "Automatic").
- File viewer width: saved as a share of the window (`file_viewer_fraction`, 0-1 in the
  file; the 240 px minimums for the viewer and the terminal still apply) when a divider drag
  ends or the divider is double-clicked, not on the settings page. Another running instance
  shows it only after it reloads the settings file (see above). While the file is broken the
  width isn't saved (the file is left for the user to fix) and lasts until the file is read
  again. A drag whose release the app misses (a right or middle button released outside the
  window, or on Windows a focus change such as Alt+Tab mid-drag) keeps the dragged width and
  saves it at the next mouse release anywhere; it is lost only if the app quits first.
- File explorer width: dragged by the 6 px strip on the explorer's right edge (double-click
  for 300 px), kept between 150 and 600 px whatever the window size (a narrow window can
  leave the terminal and viewer below their usual minimums), and saved in pixels
  (`file_explorer_width`) with the same rules as the viewer width: once when the drag ends,
  not on the settings page, not while the file is broken, other instances after a reload.
- File explorer dimming of git-ignored entries: git is asked again for a folder only when it is
  read again (expanded, or its entries change), and for every open folder when a `.gitignore`
  in one of them changes; a change to `.git/info/exclude` or the global excludes file, or a
  file starting or stopping being tracked
  shows once the folder is collapsed and expanded again. Each check gives up after 5 s (nothing
  dimmed in that folder). Inside a nested repository or submodule, git answers for that
  repository. A name that isn't valid UTF-8 is sent to git as displayed and isn't dimmed.
- File explorer and viewer switch (`file_panes`): turning it off closes every open file, so a
  tab's viewer starts empty after turning it on again (the explorer stays hidden until its
  button or Ctrl+Shift+| is used). `tabs.json` never held a tab's open file, and no file
  associations are saved anywhere, so nothing of either is kept. The unsaved-edit prompt is
  the viewer's Discard/Cancel one (there is no Save choice), asked once per file; a pending
  chain blocks a second one, and Cancel (or Esc) ends it.
  The switch is written off and then back on while the prompt is open, so
  settings.json briefly reads off (a second running instance or a reload may read it), a hand
  edit to off is undone without a message, and if the write-back fails the switch may stay off
  until the next change. With the panes off a Ctrl+clicked file link asks "Open externally?"
  (system handler) and "Open settings file" opens the file externally.
- `"keybindings"` apply when the settings file is read again (settings page opened, file saved
  in the file viewer, a change on the page), not on an edit made elsewhere alone. A key name
  GPUI doesn't know (`ctrl-foo`) is accepted
  but never matches; a `"keybindings"` value that isn't an object is ignored like any mistyped
  setting and dropped at the next save. Ctrl+C, Tab and Shift+Tab bound to a workspace action
  still go to the program while the terminal has the keyboard. Ctrl+Shift+| (explorer) is
  bound as `ctrl-|`: GPUI on Linux reports it without Shift, so `ctrl-shift-|` never matches;
  hints and the Keyboard page show it as Ctrl+|.
- Keyboard settings page: recording takes one keystroke, not a sequence (`ctrl-k ctrl-c` can
  only be written in the file; a sequence from the file shows and can be removed or reset).
  Plain Esc cancels recording, so it can't be recorded (Shift+Esc and the like can; Close find
  gets plain Esc back with Reset). Keys that type or edit text without Ctrl, Alt or Super
  (letters, Space, Enter, arrows) and Ctrl or Alt plus a letter alone (Ctrl+D, Alt+B) are
  refused, so Ctrl+V can only come back with Reset. A key already used is taken from its action
  with a toast naming it; a key only reaches a workspace action from the terminal if the
  terminal doesn't keep it (Ctrl+C, Tab, Shift+Tab). Row descriptions elsewhere on the
  settings page and README name the default keys. Recording ends on a click outside the
  recording chip, Esc or the keystroke; if the chip is hidden another way (no click), the next
  keystroke ends it and goes on as usual.
- Rename tab (F2) runs only while the tab list has the keyboard: another key for it, from the
  Keyboard page or the file, is bound in the list too, never window-wide. A key runs one action
  whatever the context, so giving F2 to another action (for the whole window) takes it from
  Rename tab, and the terminal then no longer gets F2.
- Hover cards are held back only for the menus the sidebar and the file explorer build (tab,
  group, `+`, explorer entry menus) and for any dialog: a dropdown on the settings page or in
  the file viewer doesn't hold them. A card open when a hold ends stays hidden until the
  pointer leaves its row; keeping the pointer still, the card needs a move out and back.
- Themes: an app version older than the Sepia theme reads `"ui_theme": "sepia"` as Dark (unknown
  values fall back to the default), and `"terminal_colors"` values it doesn't know as Match theme.
- Terminal colors changed while a tab runs: `COLORFGBG` keeps the value the tab started with,
  and programs that read the background color (OSC 11) only at start keep their first choice
  until restarted. Agents' own themes (Claude Code's `/theme`) are not changed.
