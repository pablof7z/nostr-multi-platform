//! Pull-cursor wake sidecar codec — ADR-0058 §4, step 3b.
//!
//! Encodes the coalesced `{cursor_id, latest_seq}` rows drained from
//! [`StoreWakeups::pull`](super::store_wakeup::StoreWakeups) into the transport
//! `PullWakeBatch` FlatBuffer and wraps them as a single
//! [`TypedProjectionData`] under the reserved key [`PULL_WAKE_KEY`]. The wake is
//! a **signal**, not event data: each row carries exactly `{ cursor_id,
//! latest_seq }` — no scope, no consumer id, no event ids, no raw bytes
//! (ADR-0058 §4). Consistent with ADR-0037 typed sidecars and ADR-0039 (this is
//! NOT a projection pull accessor; it is the change-signal that keeps pull
//! event-driven).
//!
//! The batch is encoded standalone (`finish_minimal`, no file identifier — the
//! single `NMPU` identifier belongs to `UpdateFrame`), so the payload bytes are
//! a bare finished `PullWakeBatch` ready for [`decode_pull_wake_batch`].
//!
//! ## Host decoder scope (step 3b ships the producer + bindings, not host glue)
//!
//! Step 3b lands the Rust emit, the `PullWake`/`PullWakeBatch` schema, and the
//! regenerated Swift/Kotlin/TS table accessors. It deliberately does NOT add
//! per-host typed-decoder glue (Swift `TypedProjectionDecoders`, Android frame
//! decoder, TS update-frame path), because **no host consumes `nmp.pull.wake`
//! yet** — the first consumers are the `hl` mirror (step 5) and `load_older`
//! (step 6). Wiring a decoder for a consumer that does not exist would be
//! speculative (D5); the host decoder lands with its first consumer.

use flatbuffers::FlatBufferBuilder;

use super::pull_cursor::PullCursorId;
use crate::transport::wire as fb;
use crate::update_envelope::TypedProjectionData;

/// Reserved typed-projection key / schema id for the pull-wake sidecar.
pub const PULL_WAKE_KEY: &str = "nmp.pull.wake";
/// Schema version of the `nmp.pull.wake` payload.
pub const PULL_WAKE_SCHEMA_VERSION: u32 = 1;

/// One decoded wake row — the public, language-neutral shape of a `PullWake`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PullWakeRow {
    /// The registered cursor that is behind the store head.
    pub cursor_id: u64,
    /// The store's `latest_ingest_seq` observed when the wake was armed.
    pub latest_seq: u64,
}

/// Encode the drained wake rows into a standalone `PullWakeBatch` FlatBuffer.
///
/// `wakes` is the `Vec<(PullCursorId, latest_seq)>` returned by
/// [`Kernel::drain_pull_wakes`](crate::kernel::Kernel::drain_pull_wakes).
#[must_use]
pub(crate) fn encode_pull_wake_batch(wakes: &[(PullCursorId, u64)]) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let rows: Vec<_> = wakes
        .iter()
        .map(|(cursor_id, latest_seq)| {
            fb::PullWake::create(
                &mut builder,
                &fb::PullWakeArgs {
                    cursor_id: cursor_id.0,
                    latest_seq: *latest_seq,
                },
            )
        })
        .collect();
    let wakes_vec = builder.create_vector(&rows);
    let batch = fb::PullWakeBatch::create(
        &mut builder,
        &fb::PullWakeBatchArgs {
            wakes: Some(wakes_vec),
        },
    );
    builder.finish_minimal(batch);
    builder.finished_data().to_vec()
}

/// Build the single `nmp.pull.wake` typed-projection sidecar for this tick's
/// drained wake batch. Returns `None` when there are no pending wakes (no entry
/// is emitted — an absent key means "no cursor is behind").
#[must_use]
pub(crate) fn pull_wake_typed_projection(
    wakes: &[(PullCursorId, u64)],
) -> Option<TypedProjectionData> {
    if wakes.is_empty() {
        return None;
    }
    Some(TypedProjectionData {
        key: PULL_WAKE_KEY.to_string(),
        schema_id: PULL_WAKE_KEY.to_string(),
        schema_version: PULL_WAKE_SCHEMA_VERSION,
        // Encoded standalone — no FlatBuffers file identifier (see module doc).
        file_identifier: String::new(),
        payload: encode_pull_wake_batch(wakes),
        // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
        ..Default::default()
    })
}

/// Decode a standalone `PullWakeBatch` payload into owned [`PullWakeRow`]s.
///
/// The public Rust decoder for the `nmp.pull.wake` sidecar. Returns
/// `Err(String)` on malformed input — never panics at the boundary (D6).
///
/// # Errors
/// Returns `Err` when `bytes` is not a valid `PullWakeBatch` FlatBuffer.
pub fn decode_pull_wake_batch(bytes: &[u8]) -> Result<Vec<PullWakeRow>, String> {
    let batch = flatbuffers::root::<fb::PullWakeBatch>(bytes)
        .map_err(|e| format!("invalid PullWakeBatch: {e}"))?;
    let mut out = Vec::new();
    if let Some(wakes) = batch.wakes() {
        out.reserve(wakes.len());
        for w in wakes {
            out.push(PullWakeRow {
                cursor_id: w.cursor_id(),
                latest_seq: w.latest_seq(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rows() {
        let wakes = vec![(PullCursorId(7), 42), (PullCursorId(9), 100)];
        let bytes = encode_pull_wake_batch(&wakes);
        let rows = decode_pull_wake_batch(&bytes).expect("decode");
        assert_eq!(
            rows,
            vec![
                PullWakeRow { cursor_id: 7, latest_seq: 42 },
                PullWakeRow { cursor_id: 9, latest_seq: 100 },
            ]
        );
    }

    #[test]
    fn empty_batch_yields_no_projection() {
        assert!(pull_wake_typed_projection(&[]).is_none());
    }

    #[test]
    fn projection_carries_reserved_key_and_decodes() {
        let proj = pull_wake_typed_projection(&[(PullCursorId(3), 5)]).expect("some");
        assert_eq!(proj.key, PULL_WAKE_KEY);
        assert_eq!(proj.schema_id, PULL_WAKE_KEY);
        assert_eq!(proj.schema_version, PULL_WAKE_SCHEMA_VERSION);
        let rows = decode_pull_wake_batch(&proj.payload).expect("decode");
        assert_eq!(rows, vec![PullWakeRow { cursor_id: 3, latest_seq: 5 }]);
    }

    #[test]
    fn malformed_bytes_error_not_panic() {
        assert!(decode_pull_wake_batch(&[0xff, 0x00, 0x13]).is_err());
    }
}
