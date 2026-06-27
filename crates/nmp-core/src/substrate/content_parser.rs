//! Content-parser seam: turns raw Nostr event content into NFCT
//! (`nmp-content` `ContentTreeWire`) FlatBuffers bytes.
//!
//! `nmp-core` (Layer 3) cannot depend on `nmp-content` (Layer 2 — it depends on
//! `nmp-core`), so the tokenizer cannot be called from the kernel directly.
//! Instead the kernel holds this trait object, and a higher composition layer
//! (which CAN depend on `nmp-content`) installs a real implementation via
//! `Kernel::set_content_parser`. Mirrors the `OutboxRouter` / `MailboxCache`
//! substrate seams exactly.
//!
//! `refs.event` row payloads call the installed parser to embed parsed
//! `content_tree_bytes` alongside each event ref's raw content, so a web host
//! (which cannot run `nmp-content` in JS) can render the kernel-parsed content
//! tree from an event `resolve_ref` — matching the native gallery's
//! resolve-driven content path. Hosts that install no parser (the default) get
//! an empty buffer and fall back to the raw content string — behaviour-preserving
//! for every existing native consumer (D0/D6).

/// Parses raw event content into a serialized NFCT `ContentTreeWire` buffer.
pub trait ContentParser: Send + Sync {
    /// Parse `content` (with the event's `tags`, for the given `kind`) into a
    /// serialized NFCT `ContentTreeWire` FlatBuffer (`KCTW` file identifier).
    /// Returns an empty `Vec` when there is no content tree to emit (the caller
    /// then falls back to the raw content string). Must never panic (D6).
    fn parse_to_nfct_bytes(&self, content: &str, tags: &[Vec<String>], kind: u32) -> Vec<u8>;
}

/// Default no-op parser: emits no content tree. Keeps `nmp-core` free of any
/// `nmp-content` dependency until a composition installs a real parser.
#[derive(Default)]
pub struct NoopContentParser;

impl NoopContentParser {
    /// Construct the no-op parser.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ContentParser for NoopContentParser {
    fn parse_to_nfct_bytes(&self, _content: &str, _tags: &[Vec<String>], _kind: u32) -> Vec<u8> {
        Vec::new()
    }
}
