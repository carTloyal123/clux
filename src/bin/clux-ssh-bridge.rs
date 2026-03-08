//! Test helper that forwards a local Unix socket to another Unix socket.

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        anyhow::bail!("usage: clux-ssh-bridge <local-socket> <remote-socket>");
    }

    let local_socket = &args[1];
    let remote_socket = &args[2];

    if let Some(parent) = Path::new(local_socket).parent() {
        std::fs::create_dir_all(parent)?;
    }
    if Path::new(local_socket).exists() {
        std::fs::remove_file(local_socket)?;
    }

    let listener = UnixListener::bind(local_socket)?;

    loop {
        let (local_stream, _) = listener.accept()?;
        let remote_stream = match UnixStream::connect(remote_socket) {
            Ok(stream) => stream,
            Err(_) => continue,
        };

        forward_bidirectional(local_stream, remote_stream);
    }
}

fn forward_bidirectional(local_stream: UnixStream, remote_stream: UnixStream) {
    let mut local_reader = match local_stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut local_writer = local_stream;
    let mut remote_reader = match remote_stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut remote_writer = remote_stream;

    std::thread::spawn(move || {
        let _ = io::copy(&mut local_reader, &mut remote_writer);
    });

    std::thread::spawn(move || {
        let _ = io::copy(&mut remote_reader, &mut local_writer);
    });
}
