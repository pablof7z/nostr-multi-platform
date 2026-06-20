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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConnectionReasonRow {
    /// Stable machine tag: `"nip65"` / `"hint"` / `"app_relay"` / … / `"blocked"` / `"interest"`.
    pub kind: String,
    /// Pre-formatted human label (`"Outbox of 3 people"`, `"Blocked"`, …).
    pub label: String,
    /// Semantic hue key (`"ok"` / `"warn"` / `"accent"` / `"muted"`).
    pub tone: String,
    /// Hex pubkeys of relevant authors (capped at 8).
    pub author_pubkeys: Vec<String>,
    /// Exact author total (>= `author_pubkeys.len()`). Zero when not applicable.
    pub author_total: u32,
    /// Pre-formatted kinds label (`"kind:0, kind:1"`). Non-empty for interest reasons only.
    pub kinds_label: String, // doctrine-allow: D27 — pending removal by #1677/#1678/#1680/#1681/#1682
    /// Hint origin event id (hex) when present.
    pub source_event_id: Option<String>,
}

pub(super) fn create_connection_reason<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &ConnectionReasonRow,
) -> WIPOffset<fb::RelayConnectionReason<'a>> {
    let kind = fbb.create_string(&row.kind);
    let label = fbb.create_string(&row.label);
    let tone = fbb.create_string(&row.tone);
    let pubkey_offsets: Vec<WIPOffset<&str>> = row
        .author_pubkeys
        .iter()
        .map(|pk| fbb.create_string(pk))
        .collect();
    let author_pubkeys = fbb.create_vector(&pubkey_offsets);
    let kinds_label = fbb.create_string(&row.kinds_label);
    let source_event_id = row.source_event_id.as_ref().map(|v| fbb.create_string(v));
    fb::RelayConnectionReason::create(
        fbb,
        &fb::RelayConnectionReasonArgs {
            kind: Some(kind),
            label: Some(label),
            tone: Some(tone),
            author_pubkeys: Some(author_pubkeys),
            author_total: row.author_total,
            kinds_label: Some(kinds_label),
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
    ConnectionReasonRow {
        kind: r.kind().unwrap_or_default().to_string(),
        label: r.label().unwrap_or_default().to_string(),
        tone: r.tone().unwrap_or_default().to_string(),
        author_pubkeys,
        author_total: r.author_total(),
        kinds_label: r.kinds_label().unwrap_or_default().to_string(),
        source_event_id: r
            .has_source_event_id()
            .then(|| r.source_event_id().map(str::to_string))
            .flatten(),
    }
}
