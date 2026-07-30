//! Clipboard writes via OSC 52.
//!
//! The host terminal owns the system clipboard, so clux asks it to do the copy
//! rather than talking to a window server itself:
//!
//! ```text
//! ESC ] 52 ; c ; <base64 of the text> ESC \
//! ```
//!
//! This works the same on every platform, and keeps working when the client is
//! running on the far end of an SSH session - which a native clipboard crate
//! cannot do. See AGENTS.md.
//!
//! Paste is not here: the host terminal pastes into the pty for us as bracketed
//! paste, and the client just forwards it.

use std::io::{self, Write};

/// Largest text we will hand to the host terminal in one OSC 52 sequence.
///
/// Terminals cap what they accept (and some log or ignore oversized sequences),
/// so refuse rather than emit something that will be silently dropped.
pub const MAX_COPY_BYTES: usize = 64 * 1024;

/// Errors from a clipboard write.
#[derive(Debug)]
pub enum ClipboardError {
    /// The text exceeded [`MAX_COPY_BYTES`].
    TooLarge { bytes: usize },
    /// Writing to the host terminal failed.
    Io(io::Error),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::TooLarge { bytes } => write!(
                f,
                "selection is too large to copy: {} bytes (limit {})",
                bytes, MAX_COPY_BYTES
            ),
            ClipboardError::Io(e) => write!(f, "clipboard write failed: {}", e),
        }
    }
}

impl std::error::Error for ClipboardError {}

impl From<io::Error> for ClipboardError {
    fn from(e: io::Error) -> Self {
        ClipboardError::Io(e)
    }
}

/// Build the OSC 52 sequence that copies `text` to the host clipboard.
pub fn osc52_sequence(text: &str) -> Result<String, ClipboardError> {
    if text.len() > MAX_COPY_BYTES {
        return Err(ClipboardError::TooLarge { bytes: text.len() });
    }

    Ok(format!(
        "\x1b]52;c;{}\x1b\\",
        base64_encode(text.as_bytes())
    ))
}

/// Copy `text` to the host terminal's clipboard.
pub fn copy_to_host<W: Write>(out: &mut W, text: &str) -> Result<(), ClipboardError> {
    let sequence = osc52_sequence(text)?;
    out.write_all(sequence.as_bytes())?;
    out.flush()?;
    Ok(())
}

/// Standard base64 (RFC 4648) with padding.
///
/// In-tree rather than a dependency: OSC 52 is the only thing that needs it.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);

    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(triple >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_non_ascii() {
        // Multi-byte UTF-8 must survive the round trip through the terminal.
        assert_eq!(base64_encode("é".as_bytes()), "w6k=");
        assert_eq!(base64_encode("世界".as_bytes()), "5LiW55WM");
    }

    #[test]
    fn sequence_is_a_well_formed_osc52() {
        let seq = osc52_sequence("hi").unwrap();
        assert_eq!(seq, "\x1b]52;c;aGk=\x1b\\");
    }

    #[test]
    fn oversized_copy_is_refused() {
        let big = "x".repeat(MAX_COPY_BYTES + 1);
        assert!(matches!(
            osc52_sequence(&big),
            Err(ClipboardError::TooLarge { .. })
        ));
        // Right at the limit is fine.
        assert!(osc52_sequence(&"x".repeat(MAX_COPY_BYTES)).is_ok());
    }

    #[test]
    fn copy_writes_the_sequence_to_the_terminal() {
        let mut out: Vec<u8> = Vec::new();
        copy_to_host(&mut out, "foobar").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "\x1b]52;c;Zm9vYmFy\x1b\\");
    }
}
