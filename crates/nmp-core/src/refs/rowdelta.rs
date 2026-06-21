//! ADR-0063 Lane A — owned `RefRow` / `RefRowDeltaBatch` types + FlatBuffers
//! codec for the keyed-projection row-delta carrier (`schema/ref_rowdelta.fbs`).
//!
//! The batch is the opaque `TypedPayload.payload` of a keyed `refs.*`
//! projection. Encoding/decoding is lossless and round-trips through
//! [`encode_ref_row_delta_batch`] / [`decode_ref_row_delta_batch`].

use super::wire as fb;
use flatbuffers::FlatBufferBuilder;
use std::fmt;

/// Per-row presence at ROW grain (mirrors `nmp.refs.RefRowState`).
///
/// `Unchanged` is NOT an enumerator: it is represented by ABSENCE of the row
/// from the batch (ADR-0063 invariant #1). A clear is ALWAYS explicit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RefRowState {
    /// Row's per-key `rev` advanced; `payload` present and authoritative.
    #[default]
    Changed,
    /// Row went absent (ref released). `payload` empty; host drops its cached
    /// row. Never conflated with absence.
    Cleared,
}

impl RefRowState {
    /// Map a wire `RefRowState` to the owned enum, FAILING CLOSED on an unknown
    /// discriminant. The flatc-generated enum is a `repr(transparent)` newtype
    /// over `u8`, so an out-of-range value (e.g. `state = 255` from a corrupt or
    /// future-versioned producer) reads back as `RefRowState(255)`. A naive
    /// `if == Cleared { Cleared } else { Changed }` would treat EVERY unknown
    /// value as `Changed` and commit a bogus row (fail-open). Instead we accept
    /// ONLY the two defined discriminants and reject anything else as a decode
    /// failure (D6) — the host then retains its prior cache and latches resync.
    fn try_from_wire(v: fb::RefRowState) -> Result<Self, RefRowDeltaDecodeError> {
        match v {
            fb::RefRowState::Changed => Ok(Self::Changed),
            fb::RefRowState::Cleared => Ok(Self::Cleared),
            other => Err(RefRowDeltaDecodeError::InvalidValue(format!(
                "unknown RefRowState discriminant {}",
                other.0
            ))),
        }
    }
}

impl From<RefRowState> for fb::RefRowState {
    fn from(v: RefRowState) -> Self {
        match v {
            RefRowState::Changed => fb::RefRowState::Changed,
            RefRowState::Cleared => fb::RefRowState::Cleared,
        }
    }
}

/// One owned row of a keyed reference projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefRow {
    /// Entity key: raw hex pubkey (`profile`) or event id / coordinate
    /// (`event`). Raw forms only (ADR-0032).
    pub key: String,
    /// Per-key monotonic revision (Lane B's `ref_row_rev(ns, key)`).
    pub rev: u64,
    /// Presence classification for this row in this batch.
    pub state: RefRowState,
    /// The namespace's resolved-ref typed payload bytes. Empty for `Cleared`.
    pub payload: Vec<u8>,
}

impl RefRow {
    /// Construct a `Changed` row carrying `payload` at `rev`.
    #[must_use]
    pub fn changed(key: impl Into<String>, rev: u64, payload: Vec<u8>) -> Self {
        Self {
            key: key.into(),
            rev,
            state: RefRowState::Changed,
            payload,
        }
    }

    /// Construct a payload-less `Cleared` row at `rev`.
    #[must_use]
    pub fn cleared(key: impl Into<String>, rev: u64) -> Self {
        Self {
            key: key.into(),
            rev,
            state: RefRowState::Cleared,
            payload: Vec::new(),
        }
    }
}

/// The owned form of one keyed-projection payload (`nmp.refs.RefRowDeltaBatch`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefRowDeltaBatch {
    /// Resolver namespace: `"profile"` or `"event"`.
    pub namespace: String,
    /// True iff this batch is a FULL baseline (every live row present as
    /// `Changed`). See ADR-0063 invariant #3.
    pub baseline: bool,
    /// Changed/cleared rows. A key absent here is Unchanged (invariant #1).
    pub rows: Vec<RefRow>,
}

/// Encode a [`RefRowDeltaBatch`] to a finished FlatBuffers buffer with the
/// `NRRD` file identifier.
#[must_use]
pub fn encode_ref_row_delta_batch(batch: &RefRowDeltaBatch) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let row_offsets: Vec<_> = batch
        .rows
        .iter()
        .map(|row| {
            let key = builder.create_string(&row.key);
            // A Cleared row carries no payload; omit the vector entirely so a
            // reader sees `payload() == None` (wire-stable with the schema's
            // optional `[ubyte]`).
            let payload = if row.payload.is_empty() {
                None
            } else {
                Some(builder.create_vector(&row.payload))
            };
            fb::RefRow::create(
                &mut builder,
                &fb::RefRowArgs {
                    key: Some(key),
                    rev: row.rev,
                    state: row.state.into(),
                    payload,
                },
            )
        })
        .collect();
    let rows = builder.create_vector(&row_offsets);
    let namespace = builder.create_string(&batch.namespace);
    let root = fb::RefRowDeltaBatch::create(
        &mut builder,
        &fb::RefRowDeltaBatchArgs {
            namespace: Some(namespace),
            baseline: batch.baseline,
            rows: Some(rows),
        },
    );
    fb::finish_ref_row_delta_batch_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Decode a [`RefRowDeltaBatch`] from finished FlatBuffers bytes.
pub fn decode_ref_row_delta_batch(
    bytes: &[u8],
) -> Result<RefRowDeltaBatch, RefRowDeltaDecodeError> {
    // Fail closed (D6): guard the minimum length before the FlatBuffers
    // identifier probe, which asserts on a buffer shorter than the root uoffset
    // + 4-byte file identifier (8 bytes). A truncated/garbage buffer must return
    // an error, never panic.
    const MIN_IDENTIFIED_LEN: usize = 8;
    if bytes.len() < MIN_IDENTIFIED_LEN || !fb::ref_row_delta_batch_buffer_has_identifier(bytes) {
        return Err(RefRowDeltaDecodeError::InvalidFlatbuffer(
            "missing NRRD file identifier".to_string(),
        ));
    }
    let batch = fb::root_as_ref_row_delta_batch(bytes)
        .map_err(|err| RefRowDeltaDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    let mut rows = Vec::new();
    if let Some(fb_rows) = batch.rows() {
        for index in 0..fb_rows.len() {
            let row = fb_rows.get(index);
            let key = row
                .key()
                .ok_or_else(|| {
                    RefRowDeltaDecodeError::InvalidValue(format!(
                        "ref row at index {index} missing key"
                    ))
                })?
                .to_string();
            // Fail closed (D6): an unknown `state` discriminant is a decode
            // failure, NOT a silent fall-through to `Changed`. The whole batch
            // is rejected; the host retains its prior cache.
            let state = RefRowState::try_from_wire(row.state())?;
            rows.push(RefRow {
                key,
                rev: row.rev(),
                state,
                payload: row.payload().map(|p| p.bytes().to_vec()).unwrap_or_default(),
            });
        }
    }
    Ok(RefRowDeltaBatch {
        namespace: batch.namespace().unwrap_or_default().to_string(),
        baseline: batch.baseline(),
        rows,
    })
}

/// Decode failure for [`decode_ref_row_delta_batch`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefRowDeltaDecodeError {
    /// The buffer is not a well-formed `NRRD` FlatBuffer.
    InvalidFlatbuffer(String),
    /// A field required by the carrier contract is missing.
    InvalidValue(String),
}

impl fmt::Display for RefRowDeltaDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFlatbuffer(msg) => write!(f, "invalid ref row-delta batch: {msg}"),
            Self::InvalidValue(msg) => write!(f, "invalid ref row-delta value: {msg}"),
        }
    }
}

impl std::error::Error for RefRowDeltaDecodeError {}
