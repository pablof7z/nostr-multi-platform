//! Content tokenizer — UniFFI surface (M14-C1).
//!
//! ## Core-fn provenance
//!
//! Calls `nmp_content::tokenize` / `nmp_content::tokenize_with_kind` and encodes
//! the `ContentTreeWire` result as FlatBuffers bytes via
//! `nmp_content::wire::encode_content_tree`.
//!
//! ## Output format
//!
//! Returns `Vec<u8>` — the FlatBuffers `NFCT` buffer produced by
//! `nmp_content::wire::encode_content_tree`. The same schema is used in the
//! `TypedProjection` update frames the runtime emits, so platforms that
//! already decode typed frames decode this with no new code.
//!
//! ## D6
//!
//! Invalid `content` (null in C-ABI, empty string here) returns
//! `NmpError::InvalidInput`. An unknown `mode` discriminant returns
//! `NmpError::InvalidMode`.

use nmp_content::wire::encode_content_tree;
use nmp_content::{tokenize, tokenize_with_kind, RenderMode};

use crate::stateless::NmpError;

/// Render mode passed to `tokenize_content`.
///
/// Mirrors the stateless render-mode set: Plain, Markdown, and Auto.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum ContentRenderMode {
    /// Inline tokenization only (no block-level Markdown parsing).
    Plain,
    /// Full Markdown block + inline tokenization.
    Markdown,
    /// Sniff mode by `kind`: NIP-23/NIP-54 content uses Markdown; all other
    /// kinds use plain-text inline tokenization.
    Auto,
}

impl From<ContentRenderMode> for RenderMode {
    fn from(m: ContentRenderMode) -> RenderMode {
        match m {
            ContentRenderMode::Plain => RenderMode::Plain,
            ContentRenderMode::Markdown => RenderMode::Markdown,
            ContentRenderMode::Auto => RenderMode::Auto,
        }
    }
}

/// Tokenize Nostr event content and return a FlatBuffers `ContentTreeWire` buffer.
///
/// # Arguments
///
/// * `content` — the raw event content string to tokenize.
/// * `tags`    — the event's tag array (`[[string]]`), used for NIP-30 emoji
///               resolution. Pass an empty `Vec` when the event has no tags.
/// * `mode`    — render mode (Plain / Markdown / Auto).
/// * `kind`    — event kind; only meaningful when `mode` is `Auto` (used to
///               sniff whether Markdown parsing applies).
///
/// # Returns
///
/// `Ok(Vec<u8>)` — a FlatBuffers `NFCT` buffer (schema `nmp.content.tree`,
/// file identifier `NFCT`) decodable with the generated Swift/Kotlin accessors.
///
/// `Err(NmpError::InvalidInput)` — `content` is empty.
/// `Err(NmpError::EncodeFailed)` — internal FlatBuffers encoding error (rare).
///
/// Uses the same tokenizer core as the Rust registry renderers and returns
/// FlatBuffers output.
#[uniffi::export]
pub fn tokenize_content(
    content: String,
    tags: Vec<Vec<String>>,
    mode: ContentRenderMode,
    kind: u32,
) -> Result<Vec<u8>, NmpError> {
    if content.is_empty() {
        return Err(NmpError::InvalidInput);
    }
    let render_mode: RenderMode = mode.into();
    let tree = if render_mode == RenderMode::Auto {
        tokenize_with_kind(&content, &tags, render_mode, kind)
    } else {
        tokenize(&content, &tags, render_mode)
    }
    .to_wire();

    Ok(encode_content_tree(&tree))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_content::{tokenize as core_tokenize, tokenize_with_kind as core_tokenize_with_kind};
    use nmp_content::wire::encode_content_tree as core_encode;

    // Parity: these tests verify the UniFFI fn calls the same tokenizer core
    // and produces a FlatBuffers buffer that decodes to the same
    // `ContentTreeWire` as calling the core fns directly.

    #[test]
    fn parity_plain_text_matches_core() {
        let content = "hello world";
        let uniffi_bytes =
            tokenize_content(content.to_string(), vec![], ContentRenderMode::Plain, 1).unwrap();

        // Parity: same call the C-ABI makes internally.
        let core_wire =
            core_tokenize(content, &[], nmp_content::RenderMode::Plain).to_wire();
        let expected_bytes = core_encode(&core_wire);

        assert_eq!(
            uniffi_bytes, expected_bytes,
            "UniFFI tokenize_content must produce the same FlatBuffers bytes as the core fn"
        );
    }

    #[test]
    fn parity_auto_mode_matches_core() {
        let content = "# heading\n\nsome text";
        // kind 30023 = NIP-23 long-form → Auto should pick Markdown.
        let kind = 30_023u32;
        let uniffi_bytes =
            tokenize_content(content.to_string(), vec![], ContentRenderMode::Auto, kind).unwrap();

        let core_wire = core_tokenize_with_kind(
            content,
            &[],
            nmp_content::RenderMode::Auto,
            kind,
        )
        .to_wire();
        let expected_bytes = core_encode(&core_wire);

        assert_eq!(uniffi_bytes, expected_bytes);
    }

    #[test]
    fn parity_markdown_mode_matches_core() {
        let content = "**bold** and _italic_";
        let uniffi_bytes =
            tokenize_content(content.to_string(), vec![], ContentRenderMode::Markdown, 1)
                .unwrap();

        let core_wire =
            core_tokenize(content, &[], nmp_content::RenderMode::Markdown).to_wire();
        let expected_bytes = core_encode(&core_wire);

        assert_eq!(uniffi_bytes, expected_bytes);
    }

    #[test]
    fn output_is_valid_flatbuffers_nfct_buffer() {
        // Verify the buffer starts with the FlatBuffers file identifier `NFCT`.
        let bytes =
            tokenize_content("test".to_string(), vec![], ContentRenderMode::Plain, 1).unwrap();
        // FlatBuffers file identifier is at bytes [4..8] in the buffer.
        assert!(bytes.len() >= 8, "buffer must be at least 8 bytes");
        assert_eq!(
            &bytes[4..8],
            nmp_content::wire::FILE_IDENTIFIER,
            "buffer must carry the NFCT file identifier"
        );
    }

    #[test]
    fn empty_content_returns_invalid_input_error() {
        let err = tokenize_content("".to_string(), vec![], ContentRenderMode::Plain, 1)
            .unwrap_err();
        assert!(
            matches!(err, NmpError::InvalidInput),
            "expected InvalidInput error for empty content"
        );
    }

    #[test]
    fn tags_propagate_correctly() {
        // NIP-30 custom emoji: if tags contain an emoji shortcode → resolved.
        let content = ":myemoji:";
        let tags = vec![vec![
            "emoji".to_string(),
            "myemoji".to_string(),
            "https://example.com/emoji.png".to_string(),
        ]];
        // Just verify it doesn't error; full NIP-30 resolution tests are in nmp-content.
        let bytes =
            tokenize_content(content.to_string(), tags, ContentRenderMode::Plain, 1).unwrap();
        assert!(!bytes.is_empty());
    }
}
