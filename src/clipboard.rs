//! System clipboard integration.
//!
//! Uses the arboard crate for cross-platform clipboard access.

use arboard::Clipboard;
use std::sync::Mutex;

/// Global clipboard instance (arboard requires single instance).
static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

/// Initialize the clipboard.
pub fn init() -> Result<(), arboard::Error> {
    let clipboard = Clipboard::new()?;
    *CLIPBOARD.lock().unwrap() = Some(clipboard);
    Ok(())
}

/// Copy text to the system clipboard.
pub fn copy(text: &str) -> Result<(), ClipboardError> {
    let mut guard = CLIPBOARD.lock().unwrap();
    if let Some(ref mut clipboard) = *guard {
        clipboard.set_text(text).map_err(ClipboardError::Arboard)
    } else {
        Err(ClipboardError::NotInitialized)
    }
}

/// Paste text from the system clipboard.
pub fn paste() -> Result<String, ClipboardError> {
    let mut guard = CLIPBOARD.lock().unwrap();
    if let Some(ref mut clipboard) = *guard {
        clipboard.get_text().map_err(ClipboardError::Arboard)
    } else {
        Err(ClipboardError::NotInitialized)
    }
}

/// Clipboard errors.
#[derive(Debug)]
pub enum ClipboardError {
    NotInitialized,
    Arboard(arboard::Error),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::NotInitialized => write!(f, "Clipboard not initialized"),
            ClipboardError::Arboard(e) => write!(f, "Clipboard error: {}", e),
        }
    }
}

impl std::error::Error for ClipboardError {}
