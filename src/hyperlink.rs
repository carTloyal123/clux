//! OSC 8 hyperlink storage with URL interning.
//!
//! Cells store a 4-byte [`HyperlinkId`] instead of a URL, and the id is resolved
//! back to a URL when a row is serialized for a client. For a 10k-line scrollback
//! full of links that is ~14 MB of ids instead of ~40 MB of repeated strings.
//!
//! The store is append-only for the life of a pane: URLs are interned when the
//! application emits them and dropped when the pane's terminal is dropped. There
//! is no refcounting - a pane would have to emit an unbounded number of *distinct*
//! URLs for that to matter, and the machinery to do better was dead weight.
//!
//! OSC 8 format:
//!   ESC ] 8 ; params ; URI ESC \   (open hyperlink)
//!   ESC ] 8 ; ; ESC \              (close hyperlink)

use std::collections::HashMap;
use std::sync::Arc;

use crate::cell::HyperlinkId;

/// Interned URL storage for hyperlinks.
#[derive(Debug, Default)]
pub struct HyperlinkStore {
    /// Map from URL to ID for deduplication.
    url_to_id: HashMap<Arc<str>, HyperlinkId>,
    /// Map from ID to URL for retrieval.
    id_to_url: HashMap<HyperlinkId, Arc<str>>,
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
        if let Some(&id) = self.url_to_id.get(url) {
            return id;
        }

        self.next_id += 1;
        // Ids start at 1: zero is the niche that keeps `Option<HyperlinkId>` at
        // four bytes.
        let id = HyperlinkId::new(self.next_id).expect("ids start at one");

        let url: Arc<str> = url.into();
        self.url_to_id.insert(Arc::clone(&url), id);
        self.id_to_url.insert(id, url);

        id
    }

    /// Get the URL for a hyperlink ID.
    pub fn get(&self, id: HyperlinkId) -> Option<&str> {
        self.id_to_url.get(&id).map(|s| s.as_ref())
    }
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
        // Different URL should different ID
        assert_ne!(id1, id3);

        // Can retrieve URLs
        assert_eq!(store.get(id1), Some("https://example.com"));
        assert_eq!(store.get(id3), Some("https://other.com"));
    }

    #[test]
    fn test_unknown_id() {
        let store = HyperlinkStore::new();
        assert_eq!(store.get(HyperlinkId::new(42).unwrap()), None);
    }
}
