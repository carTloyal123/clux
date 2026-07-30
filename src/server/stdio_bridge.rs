//! Bridge stdin/stdout to a Unix socket (`clux-server --stdio-bridge <socket>`).
//!
//! Remote mode normally reaches the server through `ssh -L localsock:remotesock`.
//! When that forwarding is unavailable, the client falls back to running this mode
//! over plain `ssh -T`, which turns the ssh pipe into a socket connection.
//!
//! It lives in `clux-server` rather than a helper binary because the bootstrap
//! already installs `clux-server` on the remote host: one binary, one
//! implementation, nothing to install separately. It replaced a Python script
//! that the client used to write to the remote host at runtime.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;

/// Connect to `socket_path` and pump it against this process's stdin/stdout until
/// either side closes.
pub fn run(socket_path: &Path) -> io::Result<()> {
    let socket = UnixStream::connect(socket_path)?;
    pump(socket, io::stdin(), io::stdout())
}

/// Copy `input` into the socket and the socket into `output`.
///
/// Returns when the socket closes, which is what tells ssh to tear the pipe down.
/// EOF on `input` half-closes the socket so the server sees the client leave
/// rather than hanging on to the session.
pub fn pump<R, W>(socket: UnixStream, mut input: R, mut output: W) -> io::Result<()>
where
    R: Read + Send + 'static,
    W: Write,
{
    let mut socket_writer = socket.try_clone()?;
    let mut socket_reader = socket;

    thread::spawn(move || {
        let _ = io::copy(&mut input, &mut socket_writer);
        let _ = socket_writer.shutdown(Shutdown::Write);
    });

    io::copy(&mut socket_reader, &mut output)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::sync::mpsc;

    #[test]
    fn pumps_both_directions_and_stops_when_the_socket_closes() {
        let (server_side, bridge_side) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();

        // Stand in for the clux server on the other end of the socket.
        let server = thread::spawn(move || {
            let mut reader = io::BufReader::new(server_side.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            tx.send(line).unwrap();

            let mut writer = server_side;
            writer.write_all(b"from server").unwrap();
            writer.flush().unwrap();
            // Closing the socket is what ends the bridge.
        });

        let mut output: Vec<u8> = Vec::new();
        pump(bridge_side, &b"from client\n"[..], &mut output).unwrap();
        server.join().unwrap();

        assert_eq!(rx.recv().unwrap(), "from client\n");
        assert_eq!(String::from_utf8(output).unwrap(), "from server");
    }

    #[test]
    fn connect_failure_is_reported() {
        let missing = Path::new("/tmp/clux-stdio-bridge-does-not-exist.sock");
        assert!(run(missing).is_err());
    }
}
