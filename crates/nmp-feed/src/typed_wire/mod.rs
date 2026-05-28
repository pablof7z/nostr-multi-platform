//! Typed FlatBuffers wire encoding for the feed-window envelope.
//!
//! This layer encodes ONLY the *structural* feed window — the page, the
//! cursor boundaries, and the aggregate window metrics. It deliberately
//! carries **no event cards**: those belong to the protocol layer
//! (`nmp-nip01`), whose typed wire references these tables structurally.
//! `nmp-feed` owns the outer envelope shape; the protocol layer owns the
//! cards inside it.
//!
//! Unlike the generic JSON-shaped `nmp.transport.UpdateFrame` (see
//! `nmp_core::update_envelope`), this is a *typed* projection: every field
//! is a concrete FlatBuffers slot, so hosts decode fields by name instead
//! of walking an untyped value tree.
//!
//! ## Doctrine: raw bytes only on the wire
//!
//! Event ids are raw 32-byte vectors (`[ubyte]` in the schema). No
//! `display::` helpers, no npub/bech32 encoding, no profile names. Display
//! formatting is a host concern; the wire carries raw identity bytes.
//!
//! The checked-in bindings in `generated/feed_home_generated.rs` are
//! produced by `flatc` from `schema/feed_home.fbs`. Regenerate only with
//! the workspace FlatBuffers pin (`25.12.19`):
//!
//! ```sh
//! flatc --rust --gen-all -o /tmp/nf_gen/ \
//!       crates/nmp-feed/schema/feed_home.fbs
//! # then move /tmp/nf_gen/feed_home_generated.rs into generated/
//! ```

#[allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/feed_home_generated.rs"]
mod feed_home_generated;

use feed_home_generated::nmp::feed as fb;
use flatbuffers::FlatBufferBuilder;

/// Stable projection identifier this wire shape projects into.
pub const FEED_WINDOW_SCHEMA_ID: &str = "nmp.feed.window";

/// FlatBuffers file identifier for a `FeedWindowMetrics` root buffer.
pub const FEED_WINDOW_FILE_IDENTIFIER: &[u8; 4] = b"NFWM";

/// Schema version of the typed feed-window payload. Bump on any breaking
/// field change.
pub const FEED_WINDOW_SCHEMA_VERSION: u32 = 1;

/// Wire-serializable form of a feed cursor: the oldest visible item's
/// timestamp plus its raw 32-byte event id (`None` when unanchored).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedCursorWire {
    pub oldest_created_at: i64,
    pub oldest_event_id: Option<[u8; 32]>,
}

/// Wire-serializable form of the feed page boundaries. Carries no cards —
/// only the structural window shape (start/end cursors + terminal flag).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedPageWire {
    pub start_cursor: FeedCursorWire,
    pub end_cursor: FeedCursorWire,
    pub is_complete: bool,
}

/// Wire-serializable form of the aggregate feed-window metrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedWindowMetricsWire {
    pub total_items: u32,
    pub window_limit: u32,
    pub window_offset: u32,
    pub requested_limit: u32,
}

impl Default for FeedWindowMetricsWire {
    fn default() -> Self {
        // Mirror the FlatBuffers schema defaults so a round-tripped empty
        // buffer equals `Default::default()`.
        Self {
            total_items: 0,
            window_limit: 50,
            window_offset: 0,
            requested_limit: 50,
        }
    }
}

impl From<&crate::FeedCursor> for FeedCursorWire {
    /// Convert the in-memory [`crate::FeedCursor`] into its wire form.
    ///
    /// The in-memory cursor stores the event id as a hex `String`; the wire
    /// carries raw bytes. A malformed or non-32-byte hex id decodes to
    /// `None` (unanchored) rather than failing — display/identity formatting
    /// is a host concern and the wire stays forgiving.
    fn from(cursor: &crate::FeedCursor) -> Self {
        Self {
            // `FeedCursor.created_at` is a `u64`; clamp into the wire's `i64`
            // (timestamps are far below `i64::MAX`, so this is lossless in
            // practice and saturates rather than wrapping on overflow).
            oldest_created_at: i64::try_from(cursor.created_at).unwrap_or(i64::MAX),
            oldest_event_id: decode_event_id_hex(&cursor.id),
        }
    }
}

// NOTE: There is intentionally no `From<&crate::FeedWindowMetrics>` impl.
// The in-memory `FeedWindowMetrics` carries only `{ make_window_us }` — a
// timing instrument with ZERO field overlap with the wire's
// `{ total_items, window_limit, window_offset, requested_limit }`. A `From`
// impl would be vacuous (it could only emit defaults), which violates the
// repo's zero-tolerance-on-hacks rule. The projection layer constructs
// `FeedWindowMetricsWire` directly from the live window state instead.

/// Decode a lowercase/uppercase hex string into a fixed 32-byte event id.
/// Returns `None` for any input that is not exactly 64 hex digits.
fn decode_event_id_hex(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in bytes.chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

const fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Encode a feed cursor as a child FlatBuffers `FeedCursor` table.
fn encode_cursor<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    cursor: &FeedCursorWire,
) -> flatbuffers::WIPOffset<fb::FeedCursor<'bldr>> {
    let oldest_event_id = cursor
        .oldest_event_id
        .as_ref()
        .map(|id| builder.create_vector(id));
    fb::FeedCursor::create(
        builder,
        &fb::FeedCursorArgs {
            oldest_created_at: cursor.oldest_created_at,
            oldest_event_id,
        },
    )
}

/// Encode a feed-window-metrics value as one typed FlatBuffers
/// `FeedWindowMetrics` root buffer with the `NFWM` file identifier.
#[must_use]
pub fn encode_feed_window_metrics(metrics: &FeedWindowMetricsWire) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let root = fb::FeedWindowMetrics::create(
        &mut builder,
        &fb::FeedWindowMetricsArgs {
            total_items: metrics.total_items,
            window_limit: metrics.window_limit,
            window_offset: metrics.window_offset,
            requested_limit: metrics.requested_limit,
        },
    );
    fb::finish_feed_window_metrics_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Decode a typed FlatBuffers `FeedWindowMetrics` root buffer back into the
/// owned [`FeedWindowMetricsWire`]. Returns a human-readable error string on
/// a missing identifier or malformed buffer.
pub fn decode_feed_window_metrics(bytes: &[u8]) -> Result<FeedWindowMetricsWire, String> {
    if !fb::feed_window_metrics_buffer_has_identifier(bytes) {
        return Err("missing NFWM file identifier".to_string());
    }
    let metrics = fb::root_as_feed_window_metrics(bytes).map_err(|err| format!("{err:?}"))?;
    Ok(FeedWindowMetricsWire {
        total_items: metrics.total_items(),
        window_limit: metrics.window_limit(),
        window_offset: metrics.window_offset(),
        requested_limit: metrics.requested_limit(),
    })
}

/// Encode a feed page as one typed FlatBuffers `FeedPage` root buffer.
///
/// `FeedPage` is not the schema's `root_type` (that's `FeedWindowMetrics`,
/// which owns the `NFWM` identifier), so the page buffer carries no file
/// identifier; decode it with [`decode_feed_page`].
#[must_use]
pub fn encode_feed_page(page: &FeedPageWire) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let start_cursor = encode_cursor(&mut builder, &page.start_cursor);
    let end_cursor = encode_cursor(&mut builder, &page.end_cursor);
    let root = fb::FeedPage::create(
        &mut builder,
        &fb::FeedPageArgs {
            start_cursor: Some(start_cursor),
            end_cursor: Some(end_cursor),
            is_complete: page.is_complete,
        },
    );
    builder.finish_minimal(root);
    builder.finished_data().to_vec()
}

/// Decode a typed FlatBuffers `FeedPage` root buffer back into the owned
/// [`FeedPageWire`]. Returns a human-readable error on a malformed buffer.
pub fn decode_feed_page(bytes: &[u8]) -> Result<FeedPageWire, String> {
    let page =
        flatbuffers::root::<fb::FeedPage<'_>>(bytes).map_err(|err| format!("{err:?}"))?;
    Ok(FeedPageWire {
        start_cursor: decode_cursor(page.start_cursor())?,
        end_cursor: decode_cursor(page.end_cursor())?,
        is_complete: page.is_complete(),
    })
}

/// Decode an optional child `FeedCursor` table. A missing cursor table
/// decodes to the default (unanchored) cursor.
fn decode_cursor(cursor: Option<fb::FeedCursor<'_>>) -> Result<FeedCursorWire, String> {
    let Some(cursor) = cursor else {
        return Ok(FeedCursorWire::default());
    };
    Ok(FeedCursorWire {
        oldest_created_at: cursor.oldest_created_at(),
        oldest_event_id: cursor
            .oldest_event_id()
            .map(|v| array_32(v.bytes(), "oldest_event_id"))
            .transpose()?,
    })
}

/// Convert a wire byte slice into a fixed 32-byte event id, rejecting any
/// slice whose length is not exactly 32.
fn array_32(bytes: &[u8], field: &str) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| format!("{field} must be 32 bytes, got {}", bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_constants_are_stable() {
        assert_eq!(FEED_WINDOW_SCHEMA_ID, "nmp.feed.window");
        assert_eq!(FEED_WINDOW_FILE_IDENTIFIER, b"NFWM");
        assert_eq!(FEED_WINDOW_SCHEMA_VERSION, 1);
    }

    #[test]
    fn metrics_round_trips() {
        let metrics = FeedWindowMetricsWire {
            total_items: 137,
            window_limit: 80,
            window_offset: 40,
            requested_limit: 100,
        };
        let encoded = encode_feed_window_metrics(&metrics);
        assert!(!encoded.is_empty(), "encoded must not be empty");
        let decoded = decode_feed_window_metrics(&encoded).expect("must decode");
        assert_eq!(decoded, metrics, "metrics must round-trip losslessly");
    }

    #[test]
    fn metrics_default_round_trips() {
        let decoded = decode_feed_window_metrics(&encode_feed_window_metrics(
            &FeedWindowMetricsWire::default(),
        ))
        .expect("decode");
        assert_eq!(decoded, FeedWindowMetricsWire::default());
    }

    #[test]
    fn metrics_buffer_carries_nfwm_identifier() {
        let encoded = encode_feed_window_metrics(&FeedWindowMetricsWire::default());
        assert!(
            fb::feed_window_metrics_buffer_has_identifier(&encoded),
            "buffer must carry the NFWM identifier"
        );
        // The identifier lives at bytes 4..8 of a finished FlatBuffer.
        assert_eq!(&encoded[4..8], FEED_WINDOW_FILE_IDENTIFIER);
    }

    #[test]
    fn decode_metrics_rejects_buffer_without_identifier() {
        let err = decode_feed_window_metrics(&[0u8; 16]).expect_err("must reject");
        assert!(err.contains("NFWM"), "error names the missing id: {err}");
    }

    #[test]
    fn page_round_trips() {
        let page = FeedPageWire {
            start_cursor: FeedCursorWire {
                oldest_created_at: 1_700_000_500,
                oldest_event_id: Some([7u8; 32]),
            },
            end_cursor: FeedCursorWire {
                oldest_created_at: 1_700_000_000,
                oldest_event_id: Some([9u8; 32]),
            },
            is_complete: true,
        };
        let decoded = decode_feed_page(&encode_feed_page(&page)).expect("decode");
        assert_eq!(decoded, page, "page must round-trip losslessly");
    }

    #[test]
    fn page_default_round_trips() {
        let decoded =
            decode_feed_page(&encode_feed_page(&FeedPageWire::default())).expect("decode");
        assert_eq!(decoded, FeedPageWire::default());
    }

    #[test]
    fn cursor_without_event_id_round_trips() {
        let page = FeedPageWire {
            start_cursor: FeedCursorWire {
                oldest_created_at: 42,
                oldest_event_id: None,
            },
            end_cursor: FeedCursorWire::default(),
            is_complete: false,
        };
        let decoded = decode_feed_page(&encode_feed_page(&page)).expect("decode");
        assert_eq!(decoded, page);
        assert_eq!(decoded.start_cursor.oldest_event_id, None);
    }

    #[test]
    fn feed_cursor_from_in_memory_decodes_hex_id() {
        let id_hex = "ab".repeat(32); // 64 hex chars -> 32 bytes of 0xab
        let in_memory = crate::FeedCursor {
            created_at: 1_700_000_000,
            id: id_hex,
        };
        let wire = FeedCursorWire::from(&in_memory);
        assert_eq!(wire.oldest_created_at, 1_700_000_000);
        assert_eq!(wire.oldest_event_id, Some([0xabu8; 32]));
    }

    #[test]
    fn feed_cursor_from_in_memory_bad_hex_is_unanchored() {
        let in_memory = crate::FeedCursor {
            created_at: 5,
            id: "not-hex".to_string(),
        };
        let wire = FeedCursorWire::from(&in_memory);
        assert_eq!(wire.oldest_created_at, 5);
        assert_eq!(wire.oldest_event_id, None, "bad hex decodes to unanchored");
    }
}
