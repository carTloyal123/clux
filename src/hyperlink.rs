//! Hyperlink support with URL interning.
//!
//! Implements memory-efficient OSC 8 hyperlink storage using URL interning.
//! Instead of storing full URLs per cell (50+ bytes), we store a 4-byte ID
//! that references interned URLs.
//!
//! OSC 8 format:
//!   ESC ] 8 ; params ; URI ESC \   (open hyperlink)
//!   ESC ] 8 ; ; ESC \              (close hyperlink)

// Some methods are for future use (GC, advanced parsing)
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::cell::HyperlinkId;

/// Interned URL storage for memory-efficient hyperlink support.
///
/// Memory comparison for 10k line scrollback with hyperlinks:
/// - tmux approach (full URL per cell): 40+ MB
/// - Clux interning (4-byte ID per cell): ~14 MB
#[derive(Debug, Default)]
pub struct HyperlinkStore {
    /// Map from URL to ID for deduplication.
    url_to_id: HashMap<Arc<str>, HyperlinkId>,
    /// Map from ID to URL for retrieval.
    id_to_url: HashMap<HyperlinkId, Arc<str>>,
    /// Reference counts for garbage collection.
    refcounts: HashMap<HyperlinkId, usize>,
    /// Next available ID.
    next_id: u32,
}

impl HyperlinkStore {
    /// Create a new hyperlink store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a URL and return its ID.
    /// If the URL is already interned, returns the existing ID.
    pub fn intern(&mut self, url: &str) -> HyperlinkId {
        // Check if already interned
        if let Some(&id) = self.url_to_id.get(url) {
            // Increment refcount
            *self.refcounts.entry(id).or_insert(0) += 1;
            return id;
        }

        // Create new entry
        let id = HyperlinkId(self.next_id);
        self.next_id += 1;

        let url: Arc<str> = url.into();
        self.url_to_id.insert(Arc::clone(&url), id);
        self.id_to_url.insert(id, url);
        self.refcounts.insert(id, 1);

        id
    }

    /// Get the URL for a hyperlink ID.
    pub fn get(&self, id: HyperlinkId) -> Option<&str> {
        self.id_to_url.get(&id).map(|s| s.as_ref())
    }

    /// Increment the reference count for a hyperlink.
    pub fn add_ref(&mut self, id: HyperlinkId) {
        *self.refcounts.entry(id).or_insert(0) += 1;
    }

    /// Decrement the reference count for a hyperlink.
    /// Returns true if the hyperlink was removed (refcount reached 0).
    pub fn release(&mut self, id: HyperlinkId) -> bool {
        if let Some(count) = self.refcounts.get_mut(&id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                // Remove the hyperlink
                if let Some(url) = self.id_to_url.remove(&id) {
                    self.url_to_id.remove(&url);
                }
                self.refcounts.remove(&id);
                return true;
            }
        }
        false
    }

    /// Get the number of unique URLs stored.
    pub fn len(&self) -> usize {
        self.id_to_url.len()
    }

    /// Check if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.id_to_url.is_empty()
    }

    /// Check if a URL scheme is safe to open.
    pub fn is_safe_scheme(url: &str) -> bool {
        let safe_schemes = ["http://", "https://", "file://", "mailto:"];
        safe_schemes.iter().any(|scheme| url.starts_with(scheme))
    }

    /// Open a URL in the default browser/application.
    pub fn open_url(url: &str) -> std::io::Result<()> {
        if !Self::is_safe_scheme(url) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unsafe URL scheme: {}", url),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(url).spawn()?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open").arg(url).spawn()?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn()?;
        }

        Ok(())
    }
}

/// Parse OSC 8 hyperlink parameters.
/// Format: ESC ] 8 ; id=xxx:param1=val1 ; URI ST
/// Returns (params, uri) where params is a map and uri is the URL.
pub fn parse_osc8_params(data: &[&[u8]]) -> Option<(HashMap<String, String>, String)> {
    if data.len() < 2 {
        return None;
    }

    // First segment is params (can be empty)
    let params_str = std::str::from_utf8(data.get(1)?).ok()?;
    let mut params = HashMap::new();

    for param in params_str.split(':') {
        if let Some((key, value)) = param.split_once('=') {
            params.insert(key.to_string(), value.to_string());
        }
    }

    // Second segment is the URI
    let uri = std::str::from_utf8(data.get(2)?).ok()?.to_string();

    Some((params, uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_and_get() {
        let mut store = HyperlinkStore::new();

        let id1 = store.intern("https://example.com");
        let id2 = store.intern("https://example.com");
        let id3 = store.intern("https://other.com");

        // Same URL should return same ID
        assert_eq!(id1, id2);
        // Different URL should return different ID
        assert_ne!(id1, id3);

        // Can retrieve URLs
        assert_eq!(store.get(id1), Some("https://example.com"));
        assert_eq!(store.get(id3), Some("https://other.com"));
    }

    #[test]
    fn test_refcounting() {
        let mut store = HyperlinkStore::new();

        let id = store.intern("https://example.com");
        store.intern("https://example.com"); // refcount = 2
        store.add_ref(id); // refcount = 3

        assert!(!store.release(id)); // refcount = 2
        assert!(!store.release(id)); // refcount = 1
        assert!(store.release(id)); // refcount = 0, removed

        assert_eq!(store.get(id), None);
    }

    #[test]
    fn test_safe_scheme() {
        assert!(HyperlinkStore::is_safe_scheme("https://example.com"));
        assert!(HyperlinkStore::is_safe_scheme("http://example.com"));
        assert!(HyperlinkStore::is_safe_scheme("file:///path/to/file"));
        assert!(HyperlinkStore::is_safe_scheme("mailto:user@example.com"));

        assert!(!HyperlinkStore::is_safe_scheme("javascript:alert(1)"));
        assert!(!HyperlinkStore::is_safe_scheme(
            "data:text/html,<h1>Hi</h1>"
        ));
        assert!(!HyperlinkStore::is_safe_scheme("ftp://example.com"));
    }
}
