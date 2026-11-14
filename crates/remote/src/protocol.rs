//! The messages between a client (the app) and the server, and their encoding.
//!
//! Every message is a frame: its length as a little-endian `u32`, then a tag byte and the
//! fields. Integers are little-endian, byte strings and text are a `u32` length then the
//! bytes, an optional value is a `0`/`1` byte then the value. A connection starts with each
//! side sending [`ClientMessage::Hello`] / [`ServerMessage::Hello`] with its
//! [`PROTOCOL_VERSION`]; a server that does not speak the client's version answers with
//! [`ServerMessage::Failed`] and closes the connection. The hellos hold only the version, in
//! every protocol, so that a server of another version can always say so. Past the hellos,
//! the server sends [`ServerMessage::Ping`] every 15 s and the client answers
//! [`ClientMessage::Pong`]: a connection the server hears nothing from for 45 s is dropped.

use std::io::{self, ErrorKind, Read, Write};

use term_core::TermDimensions;

/// Bumped on any change to the messages: both sides must speak the same version.
pub const PROTOCOL_VERSION: u16 = 5;

/// No frame is longer: a peer that sends one is broken (or not speaking this protocol).
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// What to start on the server.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SpawnSpec {
    /// Empty for a shell, else the agent's CLI selector (`claude`, `copilot`).
    pub kind: String,
    /// `None` starts the user's default shell.
    pub program: Option<String>,
    pub args: Vec<String>,
    /// A folder on the server; `None` is the server's home folder.
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

/// A session running on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: u64,
    /// As in [`SpawnSpec::kind`].
    pub kind: String,
    /// The pty's size.
    pub size: TermDimensions,
    pub title: Option<String>,
    /// `Some` once the program exited, with its exit code when there is one.
    pub exit: Option<Option<i32>>,
    /// The program's current folder on the server, when it can be read.
    pub folder: Option<String>,
    /// The machine name of each client attached when this was read, also for an exited
    /// session, sorted; empty for a client that did not name itself. A client whose network
    /// dropped counts until it misses the server's pings ([`ServerMessage::Ping`]) or attaches
    /// again. In `Attached`, the client attaching is not counted yet.
    pub clients: Vec<String>,
}

/// Who a client is: see [`ClientMessage::Identify`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientIdentity {
    /// Stable per install of the app: a client attaching with the id of an earlier connection
    /// replaces that connection in the session (its network dropped). Empty never matches.
    pub id: String,
    /// The client machine's name, shown by `agentws-server list`.
    pub machine: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    Hello {
        version: u16,
    },
    /// Who this client is: sent right after the hello by a client that attaches to sessions
    /// (the app), not by one that only lists or closes them. It applies to the attaches that
    /// follow it, not to earlier ones on the connection. Not in the hello itself, so a
    /// server of an older protocol still decodes the hello and reports the version mismatch.
    Identify(ClientIdentity),
    /// Start a session and attach to it: answered by [`ServerMessage::Attached`].
    Spawn {
        request: u32,
        spec: SpawnSpec,
        size: TermDimensions,
    },
    /// Show a running session here at `size`: answered by [`ServerMessage::Attached`].
    Attach {
        request: u32,
        session: u64,
        size: TermDimensions,
    },
    /// Stop sending this session's output here; it goes on running.
    Detach {
        session: u64,
    },
    /// Answered by [`ServerMessage::Sessions`].
    List {
        request: u32,
    },
    Input {
        session: u64,
        bytes: Vec<u8>,
    },
    Resize {
        session: u64,
        size: TermDimensions,
    },
    /// End the session: its program is hung up on, and it leaves the list.
    Close {
        session: u64,
    },
    /// The answer to [`ServerMessage::Ping`], sent at once.
    Pong,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerMessage {
    Hello {
        version: u16,
    },
    /// The session is attached: `snapshot` recreates its terminal at `info.size`, and its
    /// output follows in [`ServerMessage::Output`].
    Attached {
        request: u32,
        info: SessionInfo,
        snapshot: Vec<u8>,
    },
    Sessions {
        request: u32,
        sessions: Vec<SessionInfo>,
    },
    Output {
        session: u64,
        bytes: Vec<u8>,
    },
    /// Another client resized the session's pty: the output now fits that size.
    Resized {
        session: u64,
        size: TermDimensions,
    },
    Exited {
        session: u64,
        code: Option<i32>,
    },
    /// The session's program is in another folder now (`cd` in a shell).
    FolderChanged {
        session: u64,
        folder: String,
    },
    /// The request failed; `request` 0 is about the connection itself.
    Failed {
        request: u32,
        message: String,
    },
    /// Sent regularly once the hellos are exchanged: the client answers
    /// [`ClientMessage::Pong`]. A network that dropped leaves the connection open between the
    /// server and its proxy, so only a round trip through ssh shows the client is gone.
    Ping,
}

mod tag {
    pub const HELLO: u8 = 1;
    pub const SPAWN: u8 = 2;
    pub const ATTACH: u8 = 3;
    pub const DETACH: u8 = 4;
    pub const LIST: u8 = 5;
    pub const INPUT: u8 = 6;
    pub const RESIZE: u8 = 7;
    pub const CLOSE: u8 = 8;
    pub const IDENTIFY: u8 = 9;
    pub const PONG: u8 = 10;

    pub const ATTACHED: u8 = 65;
    pub const SESSIONS: u8 = 66;
    pub const OUTPUT: u8 = 67;
    pub const RESIZED: u8 = 68;
    pub const EXITED: u8 = 69;
    pub const FAILED: u8 = 70;
    pub const FOLDER_CHANGED: u8 = 71;
    pub const PING: u8 = 72;
}

impl ClientMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Encoder::new();
        match self {
            Self::Hello { version } => out.tag(tag::HELLO).u16(*version),
            Self::Identify(client) => out.tag(tag::IDENTIFY).str(&client.id).str(&client.machine),
            Self::Spawn {
                request,
                spec,
                size,
            } => {
                out.tag(tag::SPAWN).u32(*request).str(&spec.kind);
                out.opt_str(spec.program.as_deref());
                out.u32(spec.args.len() as u32);
                for arg in &spec.args {
                    out.str(arg);
                }
                out.opt_str(spec.cwd.as_deref());
                out.u32(spec.env.len() as u32);
                for (key, value) in &spec.env {
                    out.str(key).str(value);
                }
                out.size(*size)
            }
            Self::Attach {
                request,
                session,
                size,
            } => out.tag(tag::ATTACH).u32(*request).u64(*session).size(*size),
            Self::Detach { session } => out.tag(tag::DETACH).u64(*session),
            Self::List { request } => out.tag(tag::LIST).u32(*request),
            Self::Input { session, bytes } => out.tag(tag::INPUT).u64(*session).bytes(bytes),
            Self::Resize { session, size } => out.tag(tag::RESIZE).u64(*session).size(*size),
            Self::Close { session } => out.tag(tag::CLOSE).u64(*session),
            Self::Pong => out.tag(tag::PONG),
        };
        out.finish()
    }

    pub fn decode(frame: &[u8]) -> io::Result<Self> {
        let mut input = Decoder(frame);
        let message = match input.u8()? {
            tag::HELLO => Self::Hello {
                version: input.u16()?,
            },
            tag::IDENTIFY => Self::Identify(ClientIdentity {
                id: input.str()?,
                machine: input.str()?,
            }),
            tag::SPAWN => {
                let request = input.u32()?;
                let kind = input.str()?;
                let program = input.opt_str()?;
                let args = input.list(Decoder::str)?;
                let cwd = input.opt_str()?;
                let env = input.list(|input| Ok((input.str()?, input.str()?)))?;
                Self::Spawn {
                    request,
                    spec: SpawnSpec {
                        kind,
                        program,
                        args,
                        cwd,
                        env,
                    },
                    size: input.size()?,
                }
            }
            tag::ATTACH => Self::Attach {
                request: input.u32()?,
                session: input.u64()?,
                size: input.size()?,
            },
            tag::DETACH => Self::Detach {
                session: input.u64()?,
            },
            tag::LIST => Self::List {
                request: input.u32()?,
            },
            tag::INPUT => Self::Input {
                session: input.u64()?,
                bytes: input.bytes()?,
            },
            tag::RESIZE => Self::Resize {
                session: input.u64()?,
                size: input.size()?,
            },
            tag::CLOSE => Self::Close {
                session: input.u64()?,
            },
            tag::PONG => Self::Pong,
            other => return Err(invalid(format!("unknown client message {other}"))),
        };
        input.end()?;
        Ok(message)
    }
}

impl ServerMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Encoder::new();
        match self {
            Self::Hello { version } => out.tag(tag::HELLO).u16(*version),
            Self::Attached {
                request,
                info,
                snapshot,
            } => out
                .tag(tag::ATTACHED)
                .u32(*request)
                .info(info)
                .bytes(snapshot),
            Self::Sessions { request, sessions } => {
                out.tag(tag::SESSIONS).u32(*request);
                out.u32(sessions.len() as u32);
                for info in sessions {
                    out.info(info);
                }
                &mut out
            }
            Self::Output { session, bytes } => out.tag(tag::OUTPUT).u64(*session).bytes(bytes),
            Self::Resized { session, size } => out.tag(tag::RESIZED).u64(*session).size(*size),
            Self::Exited { session, code } => out.tag(tag::EXITED).u64(*session).opt_i32(*code),
            Self::Failed { request, message } => out.tag(tag::FAILED).u32(*request).str(message),
            Self::FolderChanged { session, folder } => {
                out.tag(tag::FOLDER_CHANGED).u64(*session).str(folder)
            }
            Self::Ping => out.tag(tag::PING),
        };
        out.finish()
    }

    pub fn decode(frame: &[u8]) -> io::Result<Self> {
        let mut input = Decoder(frame);
        let message = match input.u8()? {
            tag::HELLO => Self::Hello {
                version: input.u16()?,
            },
            tag::ATTACHED => Self::Attached {
                request: input.u32()?,
                info: input.info()?,
                snapshot: input.bytes()?,
            },
            tag::SESSIONS => Self::Sessions {
                request: input.u32()?,
                sessions: input.list(Decoder::info)?,
            },
            tag::OUTPUT => Self::Output {
                session: input.u64()?,
                bytes: input.bytes()?,
            },
            tag::RESIZED => Self::Resized {
                session: input.u64()?,
                size: input.size()?,
            },
            tag::EXITED => Self::Exited {
                session: input.u64()?,
                code: input.opt_i32()?,
            },
            tag::FAILED => Self::Failed {
                request: input.u32()?,
                message: input.str()?,
            },
            tag::FOLDER_CHANGED => Self::FolderChanged {
                session: input.u64()?,
                folder: input.str()?,
            },
            tag::PING => Self::Ping,
            other => return Err(invalid(format!("unknown server message {other}"))),
        };
        input.end()?;
        Ok(message)
    }
}

/// Write one encoded message as a frame. Does not flush. A frame longer than [`MAX_FRAME`],
/// which [`read_frame`] refuses, is an error and nothing is written.
pub fn write_frame(writer: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    if frame.len() > MAX_FRAME {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("frame of {} bytes", frame.len()),
        ));
    }
    writer.write_all(&(frame.len() as u32).to_le_bytes())?;
    writer.write_all(frame)
}

/// Read the next frame; `None` at the end of the stream between frames.
pub fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(invalid(format!("frame of {length} bytes")));
    }
    let mut frame = vec![0; length];
    reader.read_exact(&mut frame)?;
    Ok(Some(frame))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

struct Encoder(Vec<u8>);

impl Encoder {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn finish(self) -> Vec<u8> {
        self.0
    }

    fn tag(&mut self, tag: u8) -> &mut Self {
        self.0.push(tag);
        self
    }

    fn u16(&mut self, value: u16) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn u64(&mut self, value: u64) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.u32(bytes.len() as u32);
        self.0.extend_from_slice(bytes);
        self
    }

    fn str(&mut self, text: &str) -> &mut Self {
        self.bytes(text.as_bytes())
    }

    fn opt_str(&mut self, text: Option<&str>) -> &mut Self {
        match text {
            Some(text) => {
                self.0.push(1);
                self.str(text)
            }
            None => {
                self.0.push(0);
                self
            }
        }
    }

    fn opt_i32(&mut self, value: Option<i32>) -> &mut Self {
        match value {
            Some(value) => {
                self.0.push(1);
                self.0.extend_from_slice(&value.to_le_bytes());
            }
            None => self.0.push(0),
        }
        self
    }

    fn size(&mut self, size: TermDimensions) -> &mut Self {
        self.u16(size.columns)
            .u16(size.lines)
            .u16(size.cell_width)
            .u16(size.cell_height)
    }

    fn info(&mut self, info: &SessionInfo) -> &mut Self {
        self.u64(info.id).str(&info.kind).size(info.size);
        self.opt_str(info.title.as_deref());
        match info.exit {
            Some(code) => {
                self.0.push(1);
                self.opt_i32(code);
            }
            None => self.0.push(0),
        }
        self.opt_str(info.folder.as_deref());
        self.u32(info.clients.len() as u32);
        for machine in &info.clients {
            self.str(machine);
        }
        self
    }
}

struct Decoder<'a>(&'a [u8]);

impl Decoder<'_> {
    fn take(&mut self, count: usize) -> io::Result<&[u8]> {
        if self.0.len() < count {
            return Err(invalid("message ends early".into()));
        }
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Ok(head)
    }

    fn end(&self) -> io::Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid(format!("{} bytes after the message", self.0.len())))
        }
    }

    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn flag(&mut self) -> io::Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(invalid(format!("option marker {other}"))),
        }
    }

    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let length = self.u32()? as usize;
        Ok(self.take(length)?.to_vec())
    }

    fn str(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?).map_err(|_| invalid("text is not UTF-8".into()))
    }

    fn opt_str(&mut self) -> io::Result<Option<String>> {
        if self.flag()? {
            self.str().map(Some)
        } else {
            Ok(None)
        }
    }

    fn opt_i32(&mut self) -> io::Result<Option<i32>> {
        if self.flag()? {
            Ok(Some(i32::from_le_bytes(self.take(4)?.try_into().unwrap())))
        } else {
            Ok(None)
        }
    }

    fn list<T>(&mut self, item: impl Fn(&mut Self) -> io::Result<T>) -> io::Result<Vec<T>> {
        let count = self.u32()? as usize;
        // Every item takes at least one byte: a bogus count cannot reserve much.
        let mut items = Vec::with_capacity(count.min(self.0.len()));
        for _ in 0..count {
            items.push(item(self)?);
        }
        Ok(items)
    }

    fn size(&mut self) -> io::Result<TermDimensions> {
        Ok(TermDimensions {
            columns: self.u16()?,
            lines: self.u16()?,
            cell_width: self.u16()?,
            cell_height: self.u16()?,
        })
    }

    fn info(&mut self) -> io::Result<SessionInfo> {
        Ok(SessionInfo {
            id: self.u64()?,
            kind: self.str()?,
            size: self.size()?,
            title: self.opt_str()?,
            exit: if self.flag()? {
                Some(self.opt_i32()?)
            } else {
                None
            },
            folder: self.opt_str()?,
            clients: self.list(Decoder::str)?,
        })
    }
}

#[cfg(test)]
mod tests;
