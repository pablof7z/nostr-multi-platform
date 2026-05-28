//! Typed FlatBuffers wire encoding for the feed-window envelope.
//!
//! This layer owns only feed structure: page bounds, cursors, and window
//! metrics. Downstream timeline projections embed this
//! typed buffer instead of defining their own cursor/page tables.
//!
//! The checked-in bindings in `generated/feed_home_generated.rs` are produced
//! by `flatc` from `schema/feed_home.fbs`. Regenerate only with the workspace
//! FlatBuffers pin (`25.12.19`):
//!
//! ```sh
//! flatc --rust --gen-all -o /tmp/nf_gen/ \
//!       crates/nmp-feed/schema/feed_home.fbs
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
use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::{FeedCursor, FeedPage, FeedWindowMetrics};

/// Stable schema identifier for the structural feed-window payload.
pub const FEED_WINDOW_SCHEMA_ID: &str = "nmp.feed.window";

/// FlatBuffers file identifier for a `FeedWindow` root buffer.
pub const FEED_WINDOW_FILE_IDENTIFIER: &[u8; 4] = b"NFWM";

/// Schema version of the typed feed-window payload.
pub const FEED_WINDOW_SCHEMA_VERSION: u32 = 1;

/// Wire-serializable feed-window envelope.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedWindowWire {
    pub page: Option<FeedPage>,
    pub metrics: Option<FeedWindowMetrics>,
}

/// Encode a feed-window envelope as one typed FlatBuffers `FeedWindow` root
/// buffer with the `NFWM` file identifier.
#[must_use]
pub fn encode_feed_window(window: &FeedWindowWire) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let page = window
        .page
        .as_ref()
        .map(|page| encode_page(&mut builder, page));
    let metrics = window
        .metrics
        .as_ref()
        .map(|metrics| encode_metrics(&mut builder, metrics));
    let root = fb::FeedWindow::create(&mut builder, &fb::FeedWindowArgs { page, metrics });
    fb::finish_feed_window_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Decode a typed FlatBuffers `FeedWindow` root buffer back into owned
/// nmp-feed page/metrics types.
pub fn decode_feed_window(bytes: &[u8]) -> Result<FeedWindowWire, String> {
    if bytes.len() < 8 || !fb::feed_window_buffer_has_identifier(bytes) {
        return Err("missing NFWM file identifier".to_string());
    }
    let window = fb::root_as_feed_window(bytes).map_err(|err| format!("{err:?}"))?;
    Ok(FeedWindowWire {
        page: window.page().map(decode_page).transpose()?,
        metrics: window.metrics().map(decode_metrics),
    })
}

fn encode_page<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    page: &FeedPage,
) -> WIPOffset<fb::FeedPage<'bldr>> {
    let next_cursor = page
        .next_cursor
        .as_ref()
        .map(|cursor| encode_cursor(builder, cursor));
    fb::FeedPage::create(
        builder,
        &fb::FeedPageArgs {
            limit: page.limit as u64,
            next_cursor,
            has_more: page.has_more,
            total_blocks: page.total_blocks as u64,
        },
    )
}

fn decode_page(page: fb::FeedPage<'_>) -> Result<FeedPage, String> {
    Ok(FeedPage {
        limit: page.limit() as usize,
        next_cursor: page.next_cursor().map(decode_cursor).transpose()?,
        has_more: page.has_more(),
        total_blocks: page.total_blocks() as usize,
    })
}

fn encode_cursor<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    cursor: &FeedCursor,
) -> WIPOffset<fb::FeedCursor<'bldr>> {
    let id = builder.create_string(&cursor.id);
    fb::FeedCursor::create(
        builder,
        &fb::FeedCursorArgs {
            created_at: cursor.created_at,
            id: Some(id),
        },
    )
}

fn decode_cursor(cursor: fb::FeedCursor<'_>) -> Result<FeedCursor, String> {
    Ok(FeedCursor {
        created_at: cursor.created_at(),
        id: cursor.id().ok_or("cursor missing id")?.to_string(),
    })
}

fn encode_metrics<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    metrics: &FeedWindowMetrics,
) -> WIPOffset<fb::FeedWindowMetrics<'bldr>> {
    fb::FeedWindowMetrics::create(
        builder,
        &fb::FeedWindowMetricsArgs {
            make_window_us: metrics.make_window_us,
        },
    )
}

fn decode_metrics(metrics: fb::FeedWindowMetrics<'_>) -> FeedWindowMetrics {
    FeedWindowMetrics {
        make_window_us: metrics.make_window_us(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> FeedCursor {
        FeedCursor {
            created_at: 1_700_000_000,
            id: "a".repeat(64),
        }
    }

    #[test]
    fn constants_are_stable() {
        assert_eq!(FEED_WINDOW_SCHEMA_ID, "nmp.feed.window");
        assert_eq!(FEED_WINDOW_FILE_IDENTIFIER, b"NFWM");
        assert_eq!(FEED_WINDOW_SCHEMA_VERSION, 1);
    }

    #[test]
    fn empty_window_round_trips() {
        let window = FeedWindowWire::default();
        let decoded = decode_feed_window(&encode_feed_window(&window)).expect("decode");
        assert_eq!(decoded, window);
    }

    #[test]
    fn page_and_metrics_round_trip() {
        let window = FeedWindowWire {
            page: Some(FeedPage {
                limit: 80,
                next_cursor: Some(cursor()),
                has_more: true,
                total_blocks: 123,
            }),
            metrics: Some(FeedWindowMetrics {
                make_window_us: 456,
            }),
        };
        let decoded = decode_feed_window(&encode_feed_window(&window)).expect("decode");
        assert_eq!(decoded, window);
    }

    #[test]
    fn missing_identifier_is_rejected() {
        assert!(decode_feed_window(&[0, 1, 2, 3]).is_err());
    }
}
