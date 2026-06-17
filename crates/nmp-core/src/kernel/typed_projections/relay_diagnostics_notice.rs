//! `NoticeRow` model and its FlatBuffers encode/decode helpers,
//! extracted from `relay_diagnostics_fb` to satisfy the 500-LOC file-size gate.
//!
//! This module is a submodule of `relay_diagnostics_fb` (declared via
//! `#[path = "relay_diagnostics_notice.rs"] mod notice;` in `relay_diagnostics_fb.rs`).
//! The parent's `generated` module and FlatBuffers imports are accessible via `super::`.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::generated::nmp::kernel as fb;

/// A field-for-field mirror of one `RelayDiagnosticsNotice` entry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoticeRow {
    /// Wall-clock Unix epoch milliseconds when this NOTICE arrived.
    pub at_ms: u64,
    /// NOTICE prose (truncated to 180 chars at the capture site).
    pub text: String,
}

pub(super) fn create_notice<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &NoticeRow,
) -> WIPOffset<fb::RelayDiagnosticsNotice<'a>> {
    let text = fbb.create_string(&row.text);
    fb::RelayDiagnosticsNotice::create(
        fbb,
        &fb::RelayDiagnosticsNoticeArgs {
            at_ms: row.at_ms,
            text: Some(text),
        },
    )
}

pub(super) fn notice_from_fb(n: fb::RelayDiagnosticsNotice<'_>) -> NoticeRow {
    NoticeRow {
        at_ms: n.at_ms(),
        text: n.text().unwrap_or_default().to_string(),
    }
}
