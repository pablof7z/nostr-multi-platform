//! `ConnectionReasonRow` model and its FlatBuffers encode/decode helpers,
//! extracted from `relay_diagnostics_fb` to satisfy the 500-LOC file-size gate.
//!
//! This module is a submodule of `relay_diagnostics_fb` (declared via
//! `#[path = "relay_diagnostics_connection_reason.rs"] mod connection_reason;`
//! in `relay_diagnostics_fb.rs`).  The parent's `generated` module and
//! FlatBuffers imports are accessible via `super::`.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::generated::nmp::kernel as fb;

/// A field-for-field mirror of one `RelayConnectionReason` entry.
///
/// All fields carry raw structured data. Shells derive display labels from
/// `kind`, `author_total`, and `kinds` at render time (aim.md §4.5).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectionReasonRow {
    /// Stable machine tag: `"nip65"` / `"hint"` / `"app_relay"` / … / `"blocked"` / `"interest"`.
    pub kind: String,
    /// Semantic hue key (`"ok"` / `"warn"` / `"accent"` / `"muted"`).
    pub tone: String,
    /// Hex pubkeys of relevant authors (capped at 8).
    pub author_pubkeys: Vec<String>,
    /// Exact author total (>= `author_pubkeys.len()`). Zero when not applicable.
    pub author_total: u32,
    /// Raw kind numbers for interest reasons. Non-empty for `"interest"` reasons only.
    pub kinds: Vec<u32>,
    /// Hint origin event id (hex) when present.
    pub source_event_id: Option<String>,
}

pub(super) fn create_connection_reason<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &ConnectionReasonRow,
) -> WIPOffset<fb::RelayConnectionReason<'a>> {
    let kind = fbb.create_string(&row.kind);
    let tone = fbb.create_string(&row.tone);
    let pubkey_offsets: Vec<WIPOffset<&str>> = row
        .author_pubkeys
        .iter()
        .map(|pk| fbb.create_string(pk))
        .collect();
    let author_pubkeys = fbb.create_vector(&pubkey_offsets);
    let kinds = fbb.create_vector(&row.kinds);
    let source_event_id = row.source_event_id.as_ref().map(|v| fbb.create_string(v));
    fb::RelayConnectionReason::create(
        fbb,
        &fb::RelayConnectionReasonArgs {
            kind: Some(kind),
            tone: Some(tone),
            author_pubkeys: Some(author_pubkeys),
            author_total: row.author_total,
            kinds: Some(kinds),
            has_source_event_id: row.source_event_id.is_some(),
            source_event_id,
        },
    )
}

pub(super) fn connection_reason_from_fb(r: fb::RelayConnectionReason<'_>) -> ConnectionReasonRow {
    let mut author_pubkeys = Vec::new();
    if let Some(pks) = r.author_pubkeys() {
        author_pubkeys.reserve(pks.len());
        for pk in pks.iter() {
            author_pubkeys.push(pk.to_string());
        }
    }
    let mut kinds = Vec::new();
    if let Some(ks) = r.kinds() {
        kinds.reserve(ks.len());
        for k in ks.iter() {
            kinds.push(k);
        }
    }
    ConnectionReasonRow {
        kind: r.kind().unwrap_or_default().to_string(),
        tone: r.tone().unwrap_or_default().to_string(),
        author_pubkeys,
        author_total: r.author_total(),
        kinds,
        source_event_id: r
            .has_source_event_id()
            .then(|| r.source_event_id().map(str::to_string))
            .flatten(),
    }
}
