use super::*;

fn size() -> TermDimensions {
    TermDimensions {
        columns: 120,
        lines: 40,
        cell_width: 9,
        cell_height: 18,
    }
}

fn info() -> SessionInfo {
    SessionInfo {
        id: u64::MAX - 3,
        kind: "claude".into(),
        size: size(),
        title: Some("vim — é".into()),
        exit: Some(Some(-2)),
        folder: Some("/home/me/é project".into()),
        clients: vec!["laptop".into(), String::new(), "bureau-é".into()],
    }
}

fn client_messages() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        },
        ClientMessage::Identify(ClientIdentity {
            id: "0123456789abcdef0123456789abcdef".into(),
            machine: "LAPTOP-É".into(),
        }),
        ClientMessage::Identify(ClientIdentity {
            id: String::new(),
            machine: String::new(),
        }),
        ClientMessage::Spawn {
            request: 7,
            spec: SpawnSpec {
                kind: String::new(),
                program: Some("/bin/bash".into()),
                args: vec!["-l".into(), String::new()],
                cwd: Some("/home/me/project".into()),
                env: vec![("A".into(), "1".into()), ("B".into(), String::new())],
            },
            size: size(),
        },
        ClientMessage::Spawn {
            request: 8,
            spec: SpawnSpec::default(),
            size: size(),
        },
        ClientMessage::Attach {
            request: 9,
            session: 3,
            size: size(),
        },
        ClientMessage::Detach { session: 3 },
        ClientMessage::List { request: 10 },
        ClientMessage::Input {
            session: 3,
            bytes: b"ls\r\x00\xff".to_vec(),
        },
        ClientMessage::Resize {
            session: 3,
            size: size(),
        },
        ClientMessage::Close { session: 3 },
        ClientMessage::Pong,
        ClientMessage::Show {
            request: 11,
            session: u64::MAX,
            scrollback: true,
        },
        ClientMessage::Show {
            request: 12,
            session: 3,
            scrollback: false,
        },
    ]
}

fn server_messages() -> Vec<ServerMessage> {
    vec![
        ServerMessage::Hello {
            version: PROTOCOL_VERSION,
        },
        ServerMessage::Attached {
            request: 1,
            info: info(),
            snapshot: b"\x1b[0mhello".to_vec(),
        },
        ServerMessage::Sessions {
            request: 2,
            sessions: vec![
                info(),
                SessionInfo {
                    title: None,
                    exit: None,
                    folder: None,
                    ..info()
                },
                SessionInfo {
                    exit: Some(None),
                    clients: Vec::new(),
                    ..info()
                },
            ],
        },
        ServerMessage::Sessions {
            request: 3,
            sessions: Vec::new(),
        },
        ServerMessage::Output {
            session: 1,
            bytes: Vec::new(),
        },
        ServerMessage::Resized {
            session: 1,
            size: size(),
        },
        ServerMessage::Exited {
            session: 1,
            code: Some(0),
        },
        ServerMessage::Exited {
            session: 1,
            code: None,
        },
        ServerMessage::FolderChanged {
            session: 1,
            folder: "/srv/app".into(),
        },
        ServerMessage::Failed {
            request: 4,
            message: "no such session".into(),
        },
        ServerMessage::Ping,
        ServerMessage::Text {
            request: 5,
            lines: vec!["$ ls -é".into(), String::new(), "あい".into()],
        },
        ServerMessage::Text {
            request: 6,
            lines: Vec::new(),
        },
    ]
}

#[test]
fn client_messages_round_trip() {
    for message in client_messages() {
        assert_eq!(ClientMessage::decode(&message.encode()).unwrap(), message);
    }
}

#[test]
fn server_messages_round_trip() {
    for message in server_messages() {
        assert_eq!(ServerMessage::decode(&message.encode()).unwrap(), message);
    }
}

#[test]
fn truncated_or_padded_messages_are_refused() {
    for message in client_messages() {
        let frame = message.encode();
        for end in 0..frame.len() {
            assert!(ClientMessage::decode(&frame[..end]).is_err(), "{message:?}");
        }
        let mut padded = frame;
        padded.push(0);
        assert!(ClientMessage::decode(&padded).is_err());
    }
    for message in server_messages() {
        let frame = message.encode();
        for end in 0..frame.len() {
            assert!(ServerMessage::decode(&frame[..end]).is_err(), "{message:?}");
        }
    }
}

#[test]
fn bad_markers_and_text_are_refused() {
    assert!(ServerMessage::decode(&[200]).is_err());
    // An option marker other than 0 or 1.
    assert!(ServerMessage::decode(&[tag::EXITED, 1, 0, 0, 0, 0, 0, 0, 0, 2]).is_err());
    let mut failed = vec![tag::FAILED, 0, 0, 0, 0, 1, 0, 0, 0];
    failed.push(0xff);
    assert!(ServerMessage::decode(&failed).is_err());
}

#[test]
fn a_huge_list_count_does_not_allocate_it() {
    let frame = [tag::SESSIONS, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff];
    assert!(ServerMessage::decode(&frame).is_err());
}

#[test]
fn frames_are_length_prefixed() {
    let mut stream = Vec::new();
    write_frame(&mut stream, b"one").unwrap();
    write_frame(&mut stream, b"second").unwrap();
    let mut reader = stream.as_slice();
    assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"one");
    assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"second");
    assert_eq!(read_frame(&mut reader).unwrap(), None);
}

#[test]
fn bad_frames_are_errors() {
    // Cut inside a frame.
    let mut stream = Vec::new();
    write_frame(&mut stream, b"abcdef").unwrap();
    assert!(read_frame(&mut &stream[..7]).is_err());
    // Empty and oversized frames.
    assert!(read_frame(&mut &[0u8, 0, 0, 0][..]).is_err());
    let huge = ((MAX_FRAME + 1) as u32).to_le_bytes();
    assert!(read_frame(&mut &huge[..]).is_err());
}

#[test]
fn a_frame_the_peer_would_refuse_is_not_written() {
    let mut written = Vec::new();
    let err = write_frame(&mut written, &vec![1; MAX_FRAME + 1]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    assert!(written.is_empty());
    write_frame(&mut written, &vec![1; MAX_FRAME]).unwrap();
    assert_eq!(
        read_frame(&mut written.as_slice()).unwrap().unwrap().len(),
        MAX_FRAME
    );
}

#[test]
fn the_hello_is_only_the_version_so_any_server_decodes_it() {
    let hello = ClientMessage::Hello { version: 0x0304 }.encode();
    assert_eq!(hello, [tag::HELLO, 0x04, 0x03]);
}
