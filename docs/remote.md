# Remote sessions

The goal: run the app on one machine (e.g. a Windows laptop) and open tabs whose programs run
on another (e.g. a Linux computer). The sessions keep running when the laptop disconnects,
and the app can attach to them again, like tmux.

## Design

```
 laptop (app)                                   Linux computer
 ┌────────────────────────┐   ssh -T host       ┌──────────────────────┐   local socket   ┌─────────────────────┐
 │ Client ── RemoteSession│ ── agentws-server ──│ agentws-server proxy │ ───────────────▶ │ agentws-server serve│
 │            └ Mirror    │      proxy          │ (stdin/stdout relay) │  (this user only)│ Server ── Session(s)│
 └────────────────────────┘                     └──────────────────────┘                  └─────────────────────┘
```

- **Transport: ssh.** The app runs `ssh -T -o BatchMode=yes <host> agentws-server proxy` and
  speaks the protocol on its stdin and stdout. ssh does authentication and encryption, and no
  port is opened. ssh must log in without asking (a key or an agent): there is no terminal
  to type a password into.
- **Daemon.** `agentws-server proxy` connects to this user's daemon on a local socket
  (abstract namespace on Linux, a named pipe on Windows; peers of another user are refused),
  starting `agentws-server serve` in its own session (`setsid`) when none runs. The daemon
  exits after 60 s with no session and no connection. Sessions outlive the ssh connection.
- **Sessions on the server** are real `term_core::Session`s, so the program's terminal queries
  are answered there and shell integration can run there later. `Session::spawn_with_tap` shows
  the raw pty output (before the shell-integration filter) to the server.
- **Streaming.** The tap feeds a server-side `term_core::Mirror` and forwards the bytes to
  every attached client, both under the session's fan-out lock. Attaching takes a
  `term_core::snapshot` of that mirror and subscribes under the same lock, so the client gets
  the screen and then exactly the output after it, with no gap and nothing twice.
- **Client.** `agentws_remote::Client` holds one connection and hands out plain
  `term_core::Session`s built with `Session::remote`: the app drives them like local ones.
  Their output comes through a `SessionFeed` on the connection's reader thread; typing and
  resizing go to a `RemoteLink` that queues them for a writer thread, so the UI never waits on
  the network. A remote session never answers terminal queries: the server's session did.
  Dropping the session (closing the tab) ends it on the server; losing the connection leaves
  it running there, and the feed reports `SessionEvent::Disconnected` (not `Exited`).
  `Client::reattach` shows it again on a new connection (a snapshot, then live output), or
  finds it gone from the server's list.
- **App.** Hosts are the `remote_hosts` setting (name, ssh destination, server program),
  edited on the settings page; each gets a `+` submenu. A `LaunchSpec` with a `host` runs
  there (`remote_hosts.rs`: one connection per host, opened again when a tab starts after it
  was lost). Connecting and starting the session run on a thread of their own; the tab opens
  once the session runs, or an error toast says why (kept until dismissed). Tabs starting
  together on a host wait for one connection. Remote tabs get no shell integration or hooks
  and are not saved in `tabs.json`.
- **Reconnect.** A tab whose connection was lost shows Disconnected (an unplug badge, and a
  banner with the reason and **Reconnect**, also in its right-click menu). Reconnect
  attaches again to the session's server id (`Session::remote_id`) through
  `remote_hosts::reattach`, for all the host's disconnected tabs at once over one new
  connection, and shows each new session in place (name, color, group, place, focus and
  title kept). A session that ended there meanwhile shows "Process exited"; one gone from
  the server (closed there, or the daemon restarted) offers **Start new**: the same program
  on the same host, in its last folder. An unreachable host gives an error toast and leaves
  the tabs disconnected.
- **Folder.** After each output chunk the server reads the program's folder
  (`Session::folder_probe`: `/proc/<pid>/cwd`, one syscall) and sends `FolderChanged` when it
  differs; `SessionInfo` carries it on attach. The client keeps it in
  `Session::remote_folder`, apart from `current_folder` (a path on this machine). From a remote
  tab, agents, "New tab" and Duplicate start on the same host in that folder
  (`launcher::remote_spec`), and the tab shows it.
- **Snapshot** (`term_core::snapshot`): escape sequences that recreate the scrollback, screen,
  colors and attributes, hyperlinks, cursor (position, pending wrap, style) and modes (keypad,
  mouse, bracketed paste, focus) in a fresh terminal of the same size.
- **Slow clients.** A client with more than 32 MiB of output queued is disconnected (it can
  attach again and gets a fresh snapshot), so it never holds up the program or other clients.
- **Protocol** (`agentws_remote::protocol`): length-prefixed binary frames, a version in the
  hello; both sides must run the same protocol version. The hello holds only the version, in
  every protocol, so a server of another version can always decode it and say so.
  `Show` asks for a session's text without attaching; the server answers `Text` (its mirror's
  rows) or `Failed` for an unknown id.
- **Ping.** Past the hellos, the server sends `Ping` to each connection every 15 s and the
  client answers `Pong` at once. A connection the server hears nothing from (any message
  counts) for 45 s is dropped: it leaves every session (and `list`'s clients) and the server
  shuts its socket down, so the proxy reads the end and exits. Pinging goes from the server
  because its socket leads to the local proxy, which stays up when the network drops: only a
  round trip through ssh shows the client is gone. One watchdog thread per connection, which
  sleeps until the next ping or deadline and ends with the connection. The ping is written
  ahead of the output queued for the connection (between whole frames), so a slow link draining
  a backlog still answers in time; the 32 MiB limit above is what ends a client too far behind.
- **Client identity.** Right after the hello the app sends `Identify`: a random id made once
  per install (the file `client-id` in the config folder) and its machine name (Windows
  `COMPUTERNAME`, else the host name). A connection attaching to a session that still holds
  another connection with the same id replaces it there at once: the app lost that
  connection and reconnected, so it is counted and named once. The old connection itself is
  left to end when it misses the pings (another instance of the same install may still use
  it). `list`, `status` and `close` send no identity and never attach.

## Trying it

1. On the server (Linux): build and install `agentws-server` (`cargo build --release -p
   agentws-remote`), on the `PATH` of non-interactive ssh logins, or give its path there
   (e.g. `~/.cargo/bin/agentws-server`) as the host's server program in the settings (for a
   host not in the settings: `AGENTWS_REMOTE_SERVER` on the app's machine).
2. On the app's machine: `ssh <host> agentws-server --version` must work without asking
   anything (a key or an agent). `command not found` with the binary in `~/.cargo/bin`:
   Ubuntu's `~/.bashrc` returns early for non-interactive shells, before rustup's
   `. "$HOME/.cargo/env"` line. On the server, source it above that early return:
   `sed -i '1i . "$HOME/.cargo/env"' ~/.bashrc`. `AGENTWS_REMOTE_SERVER` only changes what
   the app runs, not a hand-typed `ssh` command.
3. Add the host on the settings page (Remote hosts) and open a tab from its `+` submenu, or
   run `agent-workspace --remote <host> [shell|claude|copilot] [folder on the host]` (a
   destination or a host's name) to open the remote tab after the usual local one.
   `agentws-server list` / `close <id>` on the server show and end its sessions (`list`
   prints one line per session: id, kind, state, attached clients, their machine names
   comma-separated (`-` for none, `?` for a client that gave none), title, tab-separated);
   `agentws-server show <id> [--scrollback]` prints a session's screen as plain text (one line
   per row, trailing blanks and trailing empty rows dropped; with `--scrollback`, its history
   first, the oldest lines dropped beyond 16 MiB), read from the server's copy of the
   terminal: it does not attach, so the pty keeps its size and `list` does not count it, and
   an exited session shows its last screen;
   `agentws-server status` shows whether the daemon runs (never starting it), with the
   installed program's version and protocol and the daemon's session count, as `key=value`
   lines (exit 0 either way). Check on the settings page runs it over ssh. With the
   remote tab selected, Claude Code or Copilot CLI in the `+` menu, "New tab" and Duplicate
   open on that host, in the folder the tab is in there.
4. Optional: to keep the daemon running instead of starting it on the first connection, install
   the systemd user unit `crates/remote/systemd/agentws-server.service` (steps in its header).

### App on Windows, server on Linux

On the Linux server:

1. Install the ssh server (`sudo apt install openssh-server`). It must listen on the network:
   `ss -tln | grep ':22 '` shows `0.0.0.0:22`. A `ListenAddress 127.0.0.1` in
   `/etc/ssh/sshd_config.d/` makes Windows get `connection refused`; after changing it, run
   `sudo systemctl daemon-reload && sudo systemctl restart ssh.socket` (Ubuntu 24.04+ builds
   `ssh.socket` from it). A connection that times out instead means a firewall
   (`sudo ufw allow ssh`).
2. Note its address: `hostname -I` (e.g. `192.168.1.79`).

On Windows, in PowerShell (the OpenSSH client comes with Windows 10 and 11):

3. Make a key and put it on the server (there is no `ssh-copy-id`):
   ```powershell
   ssh-keygen -t ed25519
   type $env:USERPROFILE\.ssh\id_ed25519.pub | ssh bobo@192.168.1.79 "cat >> ~/.ssh/authorized_keys"
   ```
   Use no passphrase, or load the key in ssh-agent: the app's ssh cannot prompt.
4. `ssh bobo@192.168.1.79 agentws-server --version` must print the version without asking
   anything (the first `ssh` asks once to accept the host key). The host for the app is that
   ssh destination, `bobo@192.168.1.79`, or an alias from `%USERPROFILE%\.ssh\config` (for a
   port or key file: the host cannot hold ssh options).
5. Optional, once the key works: turn off password logins on the server with
   `PasswordAuthentication no` in `/etc/ssh/sshd_config.d/no-password.conf`, then
   `sudo systemctl restart ssh`.

Build the app and `agentws-server` from the same commit: a protocol version mismatch is refused.

## Status

Done: protocol, server, daemon and proxy (`agentws-server`, with `list`, `show`, `close` and
`status`), client, snapshot, remote sessions in the app's tabs, from the `+` menu's host
submenus (hosts set on the settings page) or the command line (`--remote <host>`), and the
remote folder: shown on the tab, and where agents and new tabs opened from it start;
Reconnect for tabs whose connection was lost; each host's server state on the settings page
(Check).

Next steps are in [BACKLOG.md](../BACKLOG.md) ("Remote sessions").

## Limitations

- A Windows app is named by `COMPUTERNAME`: upper case and at most 15 characters, which can
  differ from the machine's full host name.
- Several clients on one session share the pty size: the latest resize wins, and the other
  clients' mirrors follow it.
- While a program uses the alternate screen, a snapshot replays only that screen: the shell's
  output from before shows again only as the program redraws after it exits.
- A snapshot does not keep the scroll region, tab stops, charsets, OSC 4/10/11 color changes
  or kitty keyboard modes.
- Terminal queries are answered with the server's default palette, not the app's colors.
- Remote tabs have no shell integration, agent status, git branch, file explorer or command
  history yet, and are not reopened at the next start. Links to files in their output open on
  this machine.
- The folder shown for a remote tab is the full path (not shortened to `~`), and is known only
  on a Linux server (read from `/proc`).
- A host's submenu always lists its default shell, Claude Code and Copilot CLI: an agent not
  installed there fails when started, with the server's message. The settings' agent launch
  arguments don't apply to remote agents (they may name files on this machine).
- Tabs opened from a host's submenu start in its home folder. From a remote tab, a shell picked
  by name in the `+` menu opens on this machine (its programs are this machine's); "New tab"
  opens the host's default shell.
- A remote agent that exits cleanly keeps its last screen: its tab doesn't go on as a shell.
- Reconnect is manual: the app does not retry a lost connection by itself.
- Closing the app ends its remote sessions, like local ones.
- An exited session stays listed until a client closes it, and keeps counting the clients
  attached to it.
- A client whose network dropped still counts in `list`'s clients for up to about 45 s (the
  ping), or until the same app attaches again (Reconnect).
- On a Windows server the named pipe cannot be shut down from another thread: a silent
  client leaves the sessions after 45 s, but its connection and proxy stay until ssh ends.
- A proxy blocked writing output to an ssh whose network dropped never reads the end of its
  socket: it stays until ssh ends, though the server dropped its connection after 45 s.
- App instances on one machine share the install id. Harmless while an instance only attaches
  its own sessions; once one can attach another's (Restore), two instances showing the same
  session would replace each other in it: the id should then be made per instance.
