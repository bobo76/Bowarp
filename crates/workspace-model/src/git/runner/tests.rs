use std::io;
use std::sync::mpsc::Sender;

use super::*;

/// Gives `data`, then blocks until `release` gets a message or disconnects: a pipe a
/// background process still holds open.
struct HeldPipe {
    data: Option<Vec<u8>>,
    release: Receiver<()>,
}

impl Read for HeldPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(data) = self.data.take() {
            buf[..data.len()].copy_from_slice(&data);
            return Ok(data.len());
        }
        let _ = self.release.recv();
        Ok(0)
    }
}

fn held_pipe(data: &[u8]) -> (HeldPipe, Sender<()>) {
    let (release, receiver) = mpsc::channel();
    let pipe = HeldPipe {
        data: Some(data.to_vec()),
        release: receiver,
    };
    (pipe, release)
}

#[test]
fn keeps_only_the_first_bytes_up_to_the_limit() {
    let output = Mutex::new(Vec::new());
    drain(&b"abcdef"[..], 4, &output);
    assert_eq!(output.into_inner().unwrap(), b"abcd");

    let output = Mutex::new(Vec::new());
    drain(&b"abc"[..], usize::MAX, &output);
    assert_eq!(output.into_inner().unwrap(), b"abc");
}

#[test]
fn a_pipe_held_open_gives_what_was_read_by_the_deadline() {
    let (pipe, release) = held_pipe(b"[main 1a2b3c4] message\n");
    let capture = Capture::start(Some(pipe), usize::MAX);
    let started = Instant::now();

    let output = capture.finish(Instant::now() + Duration::from_millis(200));

    assert_eq!(output, b"[main 1a2b3c4] message\n");
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(release);
}

#[test]
fn a_closed_pipe_is_not_waited_on() {
    let capture = Capture::start(Some(&b"done"[..]), usize::MAX);
    let started = Instant::now();

    let output = capture.finish(Instant::now() + Duration::from_secs(30));

    assert_eq!(output, b"done");
    assert!(started.elapsed() < Duration::from_secs(5));
}
