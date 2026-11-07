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
- **Client.** `agentws_remote::Client` holds one connection; each `RemoteSession` shows its
  session in a `Mirror` (a `Term` fed with bytes, which never writes back: the server's
  session already answered). Dropping a `RemoteSession` detaches; `close()` ends it.
- **Snapshot** (`term_core::snapshot`): escape sequences that recreate the scrollback, screen,
  colors and attributes, hyperlinks, cursor (position, pending wrap, style) and modes (keypad,
  mouse, bracketed paste, focus) in a fresh terminal of the same size.
- **Slow clients.** A client with more than 32 MiB of output queued is disconnected (it can
  attach again and gets a fresh snapshot), so it never holds up the program or other clients.
- **Protocol** (`agentws_remote::protocol`): length-prefixed binary frames, a version in the
  hello; both sides must run the same protocol version.

## Status

Done (milestone 1, no UI): protocol, server, daemon and proxy (`agentws-server`), client,
snapshot and mirror, with tests over socket pairs and through the real binary.

Next steps are in [BACKLOG.md](../BACKLOG.md) ("Remote sessions").

## Limitations (milestone 1)

- Several clients on one session share the pty size: the latest resize wins, and the other
  clients' mirrors follow it.
- While a program uses the alternate screen, a snapshot replays only that screen: the shell's
  output from before shows again only as the program redraws after it exits.
- A snapshot does not keep the scroll region, tab stops, charsets, OSC 4/10/11 color changes
  or kitty keyboard modes.
- Terminal queries are answered with the server's default palette, not the app's colors.
- Remote tabs have no shell integration, agent status, working folder, git sidebar or file
  explorer yet.
- An exited session stays listed until a client closes it.
