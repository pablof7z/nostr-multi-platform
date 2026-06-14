//! Typed FlatBuffers wire codec for the kernel-owned `"relay_diagnostics"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"relay_diagnostics"`: the serialisation of `relay_diagnostics_snapshot()` (a
//! `RelayDiagnosticsSnapshot` — the pre-rolled diagnostics-screen view). This
//! module adds a **typed FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection.
//!
//! ## Capture-once ownership (the #1031 struct->Model path)
//!
//! `relay_diagnostics_snapshot()` is `&self` but pre-formats wall-clock-relative
//! labels against a `now` it reads internally each call, so calling it twice in a
//! tick risks a one-second-bucket divergence between the JSON and typed forms.
//! The JSON path captures the produced `RelayDiagnosticsSnapshot` STRUCT once into
//! a per-tick `Kernel` field; the struct->Model mapping lives in
//! `builtins_diagnostics.rs` (where the `pub(super)` struct fields are reachable),
//! and this codec encodes that single captured instance — so every formatted
//! label is identical to the JSON by construction. Unlike the four drained
//! built-ins this is the #1031 struct->Model convention (a real struct survives;
//! no Value-parse needed).
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/relay_diagnostics_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const RELAY_DIAGNOSTICS_SCHEMA_ID: &str = "relay_diagnostics";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const RELAY_DIAGNOSTICS_FILE_IDENTIFIER: &[u8; 4] = b"KRDG";
/// Wire schema version. Bump on any breaking change to `relay_diagnostics.fbs`.
pub const RELAY_DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one SERIALISED `RelayDiagnosticsWireSub` row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WireSubRow {
    pub wire_id: String,
    pub short_wire_id: String,
    pub relay_url: String,
    pub filter_summary: String,
    pub state_label: String,
    pub state_tone: String,
    pub consumer_count_label: String,
    pub events_rx_display: Option<String>,
    pub eose_observed: bool,
    /// Unix epoch milliseconds when the subscription opened. 0 when unknown.
    pub opened_ms: u64,
    /// Unix epoch milliseconds of the last event; 0 when none.
    pub last_event_ms: u64,
    /// Unix epoch milliseconds when EOSE was observed; 0 when none.
    pub eose_ms: u64,
    pub close_reason: Option<String>,
}

/// A field-for-field mirror of one SERIALISED `RelayDiagnosticsRow`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelayRow {
    pub relay_url: String,
    pub short_url: String,
    pub role_label: String,
    pub role_tone: String,
    pub connection_label: String,
    pub connection_tone: String,
    pub auth_label: String,
    pub auth_tone: String,
    pub total_sub_count: u32,
    pub active_sub_count: u32,
    pub eosed_sub_count: u32,
    pub total_events_rx: u64,
    pub total_events_display: String,
    pub reconnect_count: u32,
    pub bytes_rx_display: Option<String>,
    pub bytes_tx_display: Option<String>,
    /// Unix epoch milliseconds of the last successful connect; 0 when never connected.
    pub last_connected_ms: u64,
    /// Unix epoch milliseconds of the last event received; 0 when none.
    pub last_event_ms: u64,
    pub last_notice: Option<String>,
    pub last_error: Option<String>,
    pub wire_subs: Vec<WireSubRow>,
    /// ADR-0051 — the relay's NIP-11 information document; `None` until fetched.
    pub info: Option<InfoRow>,
}

/// A field-for-field mirror of one SERIALISED `RelayDiagnosticsInfo` (ADR-0051).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InfoRow {
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub pubkey: Option<String>,
    pub contact: Option<String>,
    pub software: Option<String>,
    pub version: Option<String>,
    pub supported_nips: Vec<u32>,
    pub payment_required: Option<bool>,
    pub auth_required: Option<bool>,
    pub restricted_writes: Option<bool>,
}

/// A field-for-field mirror of one SERIALISED `RelayDiagnosticsInterest` row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InterestRow {
    pub key: String,
    pub state: String,
    pub state_tone: String,
    pub refcount: u32,
    pub cache_coverage: String,
    pub relay_urls: Vec<String>,
}

/// The `"relay_diagnostics"` read model — `{ relays, interests }`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelayDiagnosticsModel {
    pub relays: Vec<RelayRow>,
    pub interests: Vec<InterestRow>,
}

// --- encode ---------------------------------------------------------------

fn create_wire_sub<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &WireSubRow,
) -> WIPOffset<fb::RelayDiagnosticsWireSub<'a>> {
    let wire_id = fbb.create_string(&row.wire_id);
    let short_wire_id = fbb.create_string(&row.short_wire_id);
    let relay_url = fbb.create_string(&row.relay_url);
    let filter_summary = fbb.create_string(&row.filter_summary);
    let state_label = fbb.create_string(&row.state_label);
    let state_tone = fbb.create_string(&row.state_tone);
    let consumer_count_label = fbb.create_string(&row.consumer_count_label);
    let events_rx_display = row.events_rx_display.as_ref().map(|v| fbb.create_string(v));
    let close_reason = row.close_reason.as_ref().map(|v| fbb.create_string(v));
    fb::RelayDiagnosticsWireSub::create(
        fbb,
        &fb::RelayDiagnosticsWireSubArgs {
            wire_id: Some(wire_id),
            short_wire_id: Some(short_wire_id),
            relay_url: Some(relay_url),
            filter_summary: Some(filter_summary),
            state_label: Some(state_label),
            state_tone: Some(state_tone),
            consumer_count_label: Some(consumer_count_label),
            has_events_rx_display: row.events_rx_display.is_some(),
            events_rx_display,
            eose_observed: row.eose_observed,
            opened_ms: row.opened_ms,
            last_event_ms: row.last_event_ms,
            eose_ms: row.eose_ms,
            has_close_reason: row.close_reason.is_some(),
            close_reason,
        },
    )
}

fn create_info<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    info: &InfoRow,
) -> WIPOffset<fb::RelayDiagnosticsInfo<'a>> {
    let name = info.name.as_ref().map(|v| fbb.create_string(v));
    let description = info.description.as_ref().map(|v| fbb.create_string(v));
    let icon = info.icon.as_ref().map(|v| fbb.create_string(v));
    let pubkey = info.pubkey.as_ref().map(|v| fbb.create_string(v));
    let contact = info.contact.as_ref().map(|v| fbb.create_string(v));
    let software = info.software.as_ref().map(|v| fbb.create_string(v));
    let version = info.version.as_ref().map(|v| fbb.create_string(v));
    let supported_nips = fbb.create_vector(&info.supported_nips);
    fb::RelayDiagnosticsInfo::create(
        fbb,
        &fb::RelayDiagnosticsInfoArgs {
            has_name: info.name.is_some(),
            name,
            has_description: info.description.is_some(),
            description,
            has_icon: info.icon.is_some(),
            icon,
            has_pubkey: info.pubkey.is_some(),
            pubkey,
            has_contact: info.contact.is_some(),
            contact,
            has_software: info.software.is_some(),
            software,
            has_version: info.version.is_some(),
            version,
            supported_nips: Some(supported_nips),
            has_payment_required: info.payment_required.is_some(),
            payment_required: info.payment_required.unwrap_or(false),
            has_auth_required: info.auth_required.is_some(),
            auth_required: info.auth_required.unwrap_or(false),
            has_restricted_writes: info.restricted_writes.is_some(),
            restricted_writes: info.restricted_writes.unwrap_or(false),
        },
    )
}

fn create_relay_row<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &RelayRow,
) -> WIPOffset<fb::RelayDiagnosticsRow<'a>> {
    let wire_offsets: Vec<WIPOffset<fb::RelayDiagnosticsWireSub<'_>>> = row
        .wire_subs
        .iter()
        .map(|w| create_wire_sub(fbb, w))
        .collect();
    let wire_subs = fbb.create_vector(&wire_offsets);
    let info = row.info.as_ref().map(|i| create_info(fbb, i));
    let relay_url = fbb.create_string(&row.relay_url);
    let short_url = fbb.create_string(&row.short_url);
    let role_label = fbb.create_string(&row.role_label);
    let role_tone = fbb.create_string(&row.role_tone);
    let connection_label = fbb.create_string(&row.connection_label);
    let connection_tone = fbb.create_string(&row.connection_tone);
    let auth_label = fbb.create_string(&row.auth_label);
    let auth_tone = fbb.create_string(&row.auth_tone);
    let total_events_display = fbb.create_string(&row.total_events_display);
    let bytes_rx_display = row.bytes_rx_display.as_ref().map(|v| fbb.create_string(v));
    let bytes_tx_display = row.bytes_tx_display.as_ref().map(|v| fbb.create_string(v));
    let last_notice = row.last_notice.as_ref().map(|v| fbb.create_string(v));
    let last_error = row.last_error.as_ref().map(|v| fbb.create_string(v));
    fb::RelayDiagnosticsRow::create(
        fbb,
        &fb::RelayDiagnosticsRowArgs {
            relay_url: Some(relay_url),
            short_url: Some(short_url),
            role_label: Some(role_label),
            role_tone: Some(role_tone),
            connection_label: Some(connection_label),
            connection_tone: Some(connection_tone),
            auth_label: Some(auth_label),
            auth_tone: Some(auth_tone),
            total_sub_count: row.total_sub_count,
            active_sub_count: row.active_sub_count,
            eosed_sub_count: row.eosed_sub_count,
            total_events_rx: row.total_events_rx,
            total_events_display: Some(total_events_display),
            reconnect_count: row.reconnect_count,
            has_bytes_rx_display: row.bytes_rx_display.is_some(),
            bytes_rx_display,
            has_bytes_tx_display: row.bytes_tx_display.is_some(),
            bytes_tx_display,
            last_connected_ms: row.last_connected_ms,
            last_event_ms: row.last_event_ms,
            has_last_notice: row.last_notice.is_some(),
            last_notice,
            has_last_error: row.last_error.is_some(),
            last_error,
            wire_subs: Some(wire_subs),
            info,
        },
    )
}

fn create_interest<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &InterestRow,
) -> WIPOffset<fb::RelayDiagnosticsInterest<'a>> {
    let url_offsets: Vec<WIPOffset<&str>> =
        row.relay_urls.iter().map(|u| fbb.create_string(u)).collect();
    let relay_urls = fbb.create_vector(&url_offsets);
    let key = fbb.create_string(&row.key);
    let state = fbb.create_string(&row.state);
    let state_tone = fbb.create_string(&row.state_tone);
    let cache_coverage = fbb.create_string(&row.cache_coverage);
    fb::RelayDiagnosticsInterest::create(
        fbb,
        &fb::RelayDiagnosticsInterestArgs {
            key: Some(key),
            state: Some(state),
            state_tone: Some(state_tone),
            refcount: row.refcount,
            cache_coverage: Some(cache_coverage),
            relay_urls: Some(relay_urls),
        },
    )
}

/// Encode a [`RelayDiagnosticsModel`] to typed FlatBuffers bytes (with the
/// `KRDG` file identifier).
#[must_use]
pub(crate) fn encode_relay_diagnostics(model: &RelayDiagnosticsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let relay_offsets: Vec<WIPOffset<fb::RelayDiagnosticsRow<'_>>> = model
        .relays
        .iter()
        .map(|r| create_relay_row(&mut fbb, r))
        .collect();
    let relays = fbb.create_vector(&relay_offsets);
    let interest_offsets: Vec<WIPOffset<fb::RelayDiagnosticsInterest<'_>>> = model
        .interests
        .iter()
        .map(|i| create_interest(&mut fbb, i))
        .collect();
    let interests = fbb.create_vector(&interest_offsets);
    let root = fb::RelayDiagnosticsSnapshot::create(
        &mut fbb,
        &fb::RelayDiagnosticsSnapshotArgs {
            relays: Some(relays),
            interests: Some(interests),
        },
    );
    fb::finish_relay_diagnostics_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_relay_diagnostics`])
/// back into a [`RelayDiagnosticsModel`]. Returns an error string on any
/// malformed input.
pub fn decode_relay_diagnostics(bytes: &[u8]) -> Result<RelayDiagnosticsModel, String> {
    if bytes.len() < 8 || !fb::relay_diagnostics_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KRDG file identifier".to_string());
    }
    let root = fb::root_as_relay_diagnostics_snapshot(bytes)
        .map_err(|e| format!("not a valid RelayDiagnosticsSnapshot buffer: {e}"))?;

    let mut relays = Vec::new();
    if let Some(fb_relays) = root.relays() {
        relays.reserve(fb_relays.len());
        for r in fb_relays.iter() {
            relays.push(relay_row_from_fb(r));
        }
    }
    let mut interests = Vec::new();
    if let Some(fb_interests) = root.interests() {
        interests.reserve(fb_interests.len());
        for i in fb_interests.iter() {
            interests.push(interest_from_fb(i));
        }
    }
    Ok(RelayDiagnosticsModel { relays, interests })
}

fn opt(s: Option<&str>) -> Option<String> {
    s.map(str::to_string)
}

fn wire_sub_from_fb(row: fb::RelayDiagnosticsWireSub<'_>) -> WireSubRow {
    WireSubRow {
        wire_id: row.wire_id().unwrap_or_default().to_string(),
        short_wire_id: row.short_wire_id().unwrap_or_default().to_string(),
        relay_url: row.relay_url().unwrap_or_default().to_string(),
        filter_summary: row.filter_summary().unwrap_or_default().to_string(),
        state_label: row.state_label().unwrap_or_default().to_string(),
        state_tone: row.state_tone().unwrap_or_default().to_string(),
        consumer_count_label: row.consumer_count_label().unwrap_or_default().to_string(),
        events_rx_display: row.has_events_rx_display().then(|| opt(row.events_rx_display())).flatten(),
        eose_observed: row.eose_observed(),
        opened_ms: row.opened_ms(),
        last_event_ms: row.last_event_ms(),
        eose_ms: row.eose_ms(),
        close_reason: row.has_close_reason().then(|| opt(row.close_reason())).flatten(),
    }
}

fn relay_row_from_fb(row: fb::RelayDiagnosticsRow<'_>) -> RelayRow {
    let mut wire_subs = Vec::new();
    if let Some(fb_subs) = row.wire_subs() {
        wire_subs.reserve(fb_subs.len());
        for s in fb_subs.iter() {
            wire_subs.push(wire_sub_from_fb(s));
        }
    }
    RelayRow {
        relay_url: row.relay_url().unwrap_or_default().to_string(),
        short_url: row.short_url().unwrap_or_default().to_string(),
        role_label: row.role_label().unwrap_or_default().to_string(),
        role_tone: row.role_tone().unwrap_or_default().to_string(),
        connection_label: row.connection_label().unwrap_or_default().to_string(),
        connection_tone: row.connection_tone().unwrap_or_default().to_string(),
        auth_label: row.auth_label().unwrap_or_default().to_string(),
        auth_tone: row.auth_tone().unwrap_or_default().to_string(),
        total_sub_count: row.total_sub_count(),
        active_sub_count: row.active_sub_count(),
        eosed_sub_count: row.eosed_sub_count(),
        total_events_rx: row.total_events_rx(),
        total_events_display: row.total_events_display().unwrap_or_default().to_string(),
        reconnect_count: row.reconnect_count(),
        bytes_rx_display: row.has_bytes_rx_display().then(|| opt(row.bytes_rx_display())).flatten(),
        bytes_tx_display: row.has_bytes_tx_display().then(|| opt(row.bytes_tx_display())).flatten(),
        last_connected_ms: row.last_connected_ms(),
        last_event_ms: row.last_event_ms(),
        last_notice: row.has_last_notice().then(|| opt(row.last_notice())).flatten(),
        last_error: row.has_last_error().then(|| opt(row.last_error())).flatten(),
        wire_subs,
        info: row.info().map(info_from_fb),
    }
}

fn info_from_fb(info: fb::RelayDiagnosticsInfo<'_>) -> InfoRow {
    let mut supported_nips = Vec::new();
    if let Some(nips) = info.supported_nips() {
        supported_nips.reserve(nips.len());
        for n in nips.iter() {
            supported_nips.push(n);
        }
    }
    InfoRow {
        name: info.has_name().then(|| opt(info.name())).flatten(),
        description: info.has_description().then(|| opt(info.description())).flatten(),
        icon: info.has_icon().then(|| opt(info.icon())).flatten(),
        pubkey: info.has_pubkey().then(|| opt(info.pubkey())).flatten(),
        contact: info.has_contact().then(|| opt(info.contact())).flatten(),
        software: info.has_software().then(|| opt(info.software())).flatten(),
        version: info.has_version().then(|| opt(info.version())).flatten(),
        supported_nips,
        payment_required: info.has_payment_required().then(|| info.payment_required()),
        auth_required: info.has_auth_required().then(|| info.auth_required()),
        restricted_writes: info.has_restricted_writes().then(|| info.restricted_writes()),
    }
}

fn interest_from_fb(row: fb::RelayDiagnosticsInterest<'_>) -> InterestRow {
    let mut relay_urls = Vec::new();
    if let Some(urls) = row.relay_urls() {
        relay_urls.reserve(urls.len());
        for u in urls.iter() {
            relay_urls.push(u.to_string());
        }
    }
    InterestRow {
        key: row.key().unwrap_or_default().to_string(),
        state: row.state().unwrap_or_default().to_string(),
        state_tone: row.state_tone().unwrap_or_default().to_string(),
        refcount: row.refcount(),
        cache_coverage: row.cache_coverage().unwrap_or_default().to_string(),
        relay_urls,
    }
}

#[cfg(test)]
#[path = "relay_diagnostics_fb_tests.rs"]
mod tests;
