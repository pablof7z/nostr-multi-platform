//! Diagnostics-screen projection: pre-rolled relay + wire-subscription rows.
//!
//! The three iOS diagnostics surfaces (`DiagnosticsView`, `RelayDetailView`,
//! `WireSubscriptionDetailView`) used to filter / sort / reduce the raw
//! `relay_statuses` + `wire_subscriptions` arrays client-side, format dates
//! client-side, and switch on protocol semantics (`state == "open"`) client-
//! side. All three are bible violations:
//!
//! - aim.md §4.5 "no derived state": the planner / projection layer owns
//!   roll-ups, not the shell.
//! - aim.md §6 anti-pattern #1: "Rust pre-formats timestamps … native
//!   renders them."
//! - aim.md §"Where do views live?" (line 241): "Bible rules out (c)" —
//!   views are not computed in platform code.
//!
//! This projection emits one `RelayDiagnosticsRow` per known relay URL with
//! every roll-up the diagnostics screen needs (active / EOSE'd / total subs,
//! cumulative events received, raw Unix-epoch-millisecond timestamps for
//! `last_connected_at` and `last_event_at`, pre-formatted connection /
//! auth / role labels) plus a per-wire-subscription enriched row with the
//! same treatment for the detail screen.
//!
//! Timestamp fields (`last_connected_ms`, `last_event_ms`, `opened_ms`,
//! `eose_ms`) carry Unix epoch milliseconds (u64). Shells format them as
//! "Xs ago" / "Xm ago" etc. at render time via platform helpers
//! (`relativeTimeFromUnixSeconds` on iOS, `formatRelativeTime` on Android).
//! This satisfies aim.md §62: no `format_ago_*` inside projection builders.
//!
//! Emitted under the snapshot `projections` key
//! [`RELAY_DIAGNOSTICS_PROJECTION_KEY`] (`"relay_diagnostics"`). The shell
//! decodes it as a single struct and renders fields directly: no `.filter`,
//! no `.sorted`, no `Date(timeIntervalSince1970:)`.

use serde::Serialize;
use std::collections::BTreeMap;

mod format;

use super::{Kernel, RelayStatus, WireSubscriptionStatus};
use format::{
    auth_label, auth_tone, compact_count, connection_tone, format_bytes,
    interest_state_tone, role_label, role_tone, short_id, short_relay_url, state_tone, title_case,
};

/// Snapshot-projection key under which the diagnostics roll-up is emitted.
/// Keep in sync with the Swift `SnapshotProjections.relayDiagnostics`
/// decoder in `KernelBridge.swift`. The hard-coded key in `update.rs`
/// (`"relay_diagnostics"`) is the wire string; this constant exists to make
/// the choice greppable from the projection module.
#[allow(dead_code)]
pub(super) const RELAY_DIAGNOSTICS_PROJECTION_KEY: &str = "relay_diagnostics";

/// One rolled-up row per known relay URL. Every aggregate (`active_sub_count`,
/// `eosed_sub_count`, session `total_events_rx`) is computed here. Raw Unix
/// epoch milliseconds are carried for timestamp fields; shells format them as
/// "Xs ago" / "Xm ago" at render time (aim.md §62 — no format_ago_* inside
/// projection builders).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsRow {
    /// Canonical relay URL — stable list identity.
    pub(super) relay_url: String,
    /// Pre-formatted short URL (host[/path], `ws[s]://` stripped, trailing
    /// `/` trimmed). The shell never re-derives.
    pub(super) short_url: String,
    /// Display label for the relay's role: `"Content"`, `"Indexer"`,
    /// `"Wallet"`, `"Outbox"`. Always non-empty.
    pub(super) role_label: String,
    /// Semantic role hue key — one of `"primary"`, `"write"`, `"accent"`,
    /// `"secondary"`. The shell maps it to a Color enum (UI styling is the
    /// shell's job; the *decision* of which class this row is in lives here).
    pub(super) role_tone: String,
    /// Pre-formatted connection label: `"Connected"`, `"Reconnecting"`,
    /// `"Disconnected"`, `"Unknown"`, etc.
    pub(super) connection_label: String,
    /// Semantic connection hue: `"ok" | "warn" | "error" | "muted"`.
    pub(super) connection_tone: String,
    /// Pre-formatted auth label: `"OK"`, `"Pending"`, `"Required"`, `"—"`.
    pub(super) auth_label: String,
    /// Semantic auth hue: `"ok" | "warn" | "muted"`.
    pub(super) auth_tone: String,
    /// Total wire subscriptions known to this relay.
    pub(super) total_sub_count: u32,
    /// Wire subscriptions in an active state (`open` / `live` / `active` /
    /// `opening`).
    pub(super) active_sub_count: u32,
    /// Wire subscriptions that have observed EOSE (`eose_at_ms.is_some()`).
    pub(super) eosed_sub_count: u32,
    /// Session EVENT frames received on this relay URL. This survives
    /// completed one-shot subscription eviction; `wire_subs[*].events_rx`
    /// remains the per-sub detail.
    pub(super) total_events_rx: u64,
    /// Pre-formatted total events (compact: `"1.2K"`, `"34"`).
    pub(super) total_events_display: String,
    /// Reconnect attempts since process start.
    pub(super) reconnect_count: u32,
    /// Pre-formatted "X bytes" / "Y KB" / "Z MB" label for `bytes_rx`, or
    /// `None` when the counter is zero.
    pub(super) bytes_rx_display: Option<String>,
    /// Same for `bytes_tx`.
    pub(super) bytes_tx_display: Option<String>,
    /// Unix epoch milliseconds of the last successful connect. `None` when
    /// the relay has never connected. Shells format as "Xs ago" at render time.
    pub(super) last_connected_ms: Option<u64>,
    /// Unix epoch milliseconds of the last event received. `None` when no
    /// events have arrived. Shells format as "Xs ago" at render time.
    pub(super) last_event_ms: Option<u64>,
    /// Most recent NIP-01 NOTICE prose, or `None`.
    pub(super) last_notice: Option<String>,
    /// Most recent error prose, or `None`.
    pub(super) last_error: Option<String>,
    /// Per-wire-subscription detail rows (newest by sort id last — the
    /// kernel already sorts deterministically by `wire_id`).
    pub(super) wire_subs: Vec<RelayDiagnosticsWireSub>,
    /// ADR-0051 — the relay's NIP-11 information document, once `nmp-nip11`
    /// has fetched it. `None` until the fetch resolves (or the relay serves
    /// no document). Apps read `info.name` / `info.icon` / … directly — no
    /// HTTP, no JSON, no awareness of NIP-11.
    pub(super) info: Option<RelayDiagnosticsInfo>,
}

/// Relay-information document, projected for the diagnostics surface (ADR-0051).
///
/// A field-for-field surface of the substrate-generic
/// [`crate::substrate::RelayInfoDoc`]. Carried on [`RelayDiagnosticsRow::info`]
/// so the shell renders relay name / icon / capabilities directly.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsInfo {
    /// Operator-chosen display name, when advertised.
    pub(super) name: Option<String>,
    /// Human-readable description / "about" text.
    pub(super) description: Option<String>,
    /// Relay icon URL.
    pub(super) icon: Option<String>,
    /// Operator administrative public key (hex).
    pub(super) pubkey: Option<String>,
    /// Operator contact (email / URL / nostr address).
    pub(super) contact: Option<String>,
    /// Relay software identifier.
    pub(super) software: Option<String>,
    /// Relay software version.
    pub(super) version: Option<String>,
    /// Protocol (NIP) numbers the relay advertises support for.
    pub(super) supported_nips: Vec<u32>,
    /// `limitation.payment_required`.
    pub(super) payment_required: Option<bool>,
    /// `limitation.auth_required`.
    pub(super) auth_required: Option<bool>,
    /// `limitation.restricted_writes`.
    pub(super) restricted_writes: Option<bool>,
}

impl RelayDiagnosticsInfo {
    fn from_doc(doc: &crate::substrate::RelayInfoDoc) -> Self {
        Self {
            name: doc.name.clone(),
            description: doc.description.clone(),
            icon: doc.icon.clone(),
            pubkey: doc.pubkey.clone(),
            contact: doc.contact.clone(),
            software: doc.software.clone(),
            version: doc.version.clone(),
            supported_nips: doc.supported_nips.clone(),
            payment_required: doc.limitation_payment_required,
            auth_required: doc.limitation_auth_required,
            restricted_writes: doc.limitation_restricted_writes,
        }
    }
}

/// Enriched per-subscription view for `WireSubscriptionDetailView` and the
/// list rows on `RelayDetailView`. Timestamp fields carry Unix epoch
/// milliseconds; shells format as "Xs ago" at render time (aim.md §62).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsWireSub {
    /// Full wire id (hex). Stable list identity.
    pub(super) wire_id: String,
    /// Pre-formatted short id (`"abcd1234…"`).
    pub(super) short_wire_id: String,
    /// Owning relay URL.
    pub(super) relay_url: String,
    /// Filter prose, propagated unchanged from `WireSub.filter_summary`.
    pub(super) filter_summary: String,
    /// Pre-formatted state label, e.g. `"Open"`, `"Pending"`, `"Closed"`.
    pub(super) state_label: String,
    /// Semantic state hue: `"ok" | "warn" | "muted" | "error"`.
    pub(super) state_tone: String,
    /// Pre-formatted consumer-count label, e.g. `"1 consumer"`,
    /// `"3 consumers"`. Empty string when zero consumers.
    pub(super) consumer_count_label: String,
    /// Pre-formatted events received (compact). `None` when zero.
    pub(super) events_rx_display: Option<String>,
    /// `true` iff EOSE has been observed.
    pub(super) eose_observed: bool,
    /// Unix epoch milliseconds when the subscription opened.
    /// Shells format as "Xs ago" at render time.
    pub(super) opened_ms: u64,
    /// Unix epoch milliseconds of the last event received, or `None`.
    /// Shells format as "Xs ago" at render time.
    pub(super) last_event_ms: Option<u64>,
    /// Unix epoch milliseconds when EOSE was observed, or `None`.
    /// Shells format as "Xs ago" at render time.
    pub(super) eose_ms: Option<u64>,
    /// Close reason prose (kept for the detail screen).
    pub(super) close_reason: Option<String>,
}

/// Enriched logical-interest row. The base `LogicalInterestStatus` already
/// has prose `state` / `cache_coverage` strings; we add the semantic hue
/// tone so the shell never branches on the state keyword.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsInterest {
    pub(super) key: String,
    pub(super) state: String,
    /// Semantic state hue: `"ok" | "warn" | "muted"`.
    pub(super) state_tone: String,
    pub(super) refcount: u32,
    pub(super) cache_coverage: String,
    pub(super) relay_urls: Vec<String>,
}

/// Top-level diagnostics snapshot.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsSnapshot {
    /// One row per known relay URL (typed lanes + outbox-only URLs merged).
    /// Ordered: typed lanes first (content, indexer, …) in role-enum order,
    /// then outbox-only URLs in `BTreeSet` (lexicographic) order. The shell
    /// never re-sorts.
    pub(super) relays: Vec<RelayDiagnosticsRow>,
    /// Pre-rolled interest rows — same prose as the legacy
    /// `LogicalInterestStatus` projection plus the semantic state tone.
    pub(super) interests: Vec<RelayDiagnosticsInterest>,
}

impl Kernel {
    /// Build the diagnostics roll-up. Called from
    /// `snapshot_projections_with_publish_cluster` in `update.rs`.
    pub(super) fn relay_diagnostics_snapshot(&self) -> RelayDiagnosticsSnapshot {
        // Fixed wall-clock anchor from kernel start (NO live clock read here):
        // raw ms-since-start markers are lifted to STABLE Unix-ms by adding it,
        // so an unchanged relay serializes byte-identically every 4 Hz tick (no
        // per-second churn, no per-ms jitter). Shells format at render (§62).
        let started_unix_ms = self.timing.started_unix_ms.unwrap_or(0);

        // Pre-compute statuses keyed by relay URL so each row can be filled
        // without a per-row linear scan back through `relay_statuses`.
        let statuses = self.relay_diagnostics_statuses();
        let mut by_url: BTreeMap<String, RelayStatus> = BTreeMap::new();
        let mut order: Vec<String> = Vec::with_capacity(statuses.len());
        for status in statuses {
            if !by_url.contains_key(&status.relay_url) {
                order.push(status.relay_url.clone());
            }
            by_url.insert(status.relay_url.clone(), status);
        }

        // Bucket wire-subs by relay url so we walk `self.wire.subs` exactly
        // once instead of N×M with the relay loop.
        let mut subs_by_url: BTreeMap<String, Vec<WireSubscriptionStatus>> = BTreeMap::new();
        for sub in self.wire_subscriptions() {
            subs_by_url
                .entry(sub.relay_url.clone())
                .or_default()
                .push(sub);
        }
        // Pick up any URLs that exist only in wire-subs (the kernel's
        // outbox path already lifts these into `relay_statuses`, but defend
        // against future skew so a wire sub never disappears from the UI).
        for url in subs_by_url.keys() {
            if !by_url.contains_key(url) {
                order.push(url.clone());
            }
        }

        let relays: Vec<RelayDiagnosticsRow> = order
            .into_iter()
            .map(|url| {
                let status = by_url.get(&url);
                let subs = subs_by_url.remove(&url).unwrap_or_default();
                build_relay_row(url, status, subs, started_unix_ms)
            })
            .collect();

        let interests = self
            .logical_interests()
            .into_iter()
            .map(|interest| RelayDiagnosticsInterest {
                state_tone: interest_state_tone(&interest.state).to_string(),
                key: interest.key,
                state: interest.state,
                refcount: interest.refcount,
                cache_coverage: interest.cache_coverage,
                relay_urls: interest.relay_urls,
            })
            .collect();

        RelayDiagnosticsSnapshot { relays, interests }
    }
}

/// Lift a `ms-since-kernel-start` event marker to Unix epoch ms, anchored to
/// the wall clock captured once at kernel start: `started_unix_ms + event_ms`.
///
/// Purely a function of two fixed inputs, so a given event always maps to the
/// SAME Unix timestamp no matter when the snapshot is taken — this determinism
/// is what makes the projection byte-stable (the regression this fixes).
/// Returns `None` when `event_ms == 0` (sentinel for "never observed").
fn event_to_unix_ms(started_unix_ms: u64, event_ms: u128) -> Option<u64> {
    if event_ms == 0 {
        return None;
    }
    let event_sat: u64 = event_ms.try_into().unwrap_or(u64::MAX);
    Some(started_unix_ms.saturating_add(event_sat))
}

fn build_relay_row(
    relay_url: String,
    status: Option<&RelayStatus>,
    subs: Vec<WireSubscriptionStatus>,
    started_unix_ms: u64,
) -> RelayDiagnosticsRow {
    // Synthetic row for an outbox-only URL with no `RelayStatus` lane —
    // mirrors the old Swift `syntheticRelayStatus` helper but stays Rust-
    // owned so the shell renders fields directly.
    let Some(s) = status else {
        let active_count = subs.iter().filter(|s| is_active_state(&s.state)).count();
        let connection = if active_count > 0 {
            "connected"
        } else {
            "unknown"
        };
        let last_event = subs.iter().filter_map(|s| s.last_event_at_ms).max();
        let total_events_rx = subs.iter().map(|s| s.events_rx).sum();
        return finish_row(
            relay_url,
            "outbox",
            connection,
            "—",
            0,
            None,
            last_event,
            None,
            None,
            total_events_rx,
            0,
            0,
            subs,
            started_unix_ms,
            None,
        );
    };
    let info = s.info.as_ref().map(RelayDiagnosticsInfo::from_doc);
    let (
        role,
        connection,
        auth,
        reconnect_count,
        last_connected_raw,
        last_event_raw,
        last_notice,
        last_error,
        events_rx,
        bytes_rx,
        bytes_tx,
    ) = (
        s.role.as_str(),
        s.connection.as_str(),
        s.auth.as_str(),
        s.reconnect_count,
        s.last_connected_at_ms,
        s.last_event_at_ms,
        s.last_notice.clone(),
        s.last_error.clone(),
        s.events_rx,
        s.bytes_rx,
        s.bytes_tx,
    );
    finish_row(
        relay_url,
        role,
        connection,
        auth,
        reconnect_count,
        last_connected_raw,
        last_event_raw,
        last_notice,
        last_error,
        events_rx,
        bytes_rx,
        bytes_tx,
        subs,
        started_unix_ms,
        info,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_row(
    relay_url: String,
    role: &str,
    connection: &str,
    auth: &str,
    reconnect_count: u32,
    last_connected_raw: Option<u128>,
    last_event_raw: Option<u128>,
    last_notice: Option<String>,
    last_error: Option<String>,
    events_rx: u64,
    bytes_rx: u64,
    bytes_tx: u64,
    subs: Vec<WireSubscriptionStatus>,
    started_unix_ms: u64,
    info: Option<RelayDiagnosticsInfo>,
) -> RelayDiagnosticsRow {
    let total_sub_count = subs.len() as u32;
    let active_sub_count = subs.iter().filter(|s| is_active_state(&s.state)).count() as u32;
    let eosed_sub_count = subs.iter().filter(|s| s.eose_at_ms.is_some()).count() as u32;
    let total_events_rx = events_rx;

    let wire_subs = subs
        .into_iter()
        .map(|s| build_wire_sub(s, started_unix_ms))
        .collect();

    RelayDiagnosticsRow {
        short_url: short_relay_url(&relay_url),
        relay_url,
        role_label: role_label(role),
        role_tone: role_tone(role).to_string(),
        connection_label: title_case(connection),
        connection_tone: connection_tone(connection).to_string(),
        auth_label: auth_label(auth),
        auth_tone: auth_tone(auth).to_string(),
        total_sub_count,
        active_sub_count,
        eosed_sub_count,
        total_events_rx,
        total_events_display: compact_count(total_events_rx),
        reconnect_count,
        bytes_rx_display: if bytes_rx > 0 {
            Some(format_bytes(bytes_rx))
        } else {
            None
        },
        bytes_tx_display: if bytes_tx > 0 {
            Some(format_bytes(bytes_tx))
        } else {
            None
        },
        last_connected_ms: last_connected_raw
            .and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        last_event_ms: last_event_raw
            .and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        last_notice,
        last_error,
        wire_subs,
        info,
    }
}

fn build_wire_sub(s: WireSubscriptionStatus, started_unix_ms: u64) -> RelayDiagnosticsWireSub {
    let consumer_count_label = match s.logical_consumer_count {
        0 => String::new(),
        1 => "1 consumer".to_string(),
        n => format!("{n} consumers"),
    };
    let events_rx_display = if s.events_rx > 0 {
        Some(compact_count(s.events_rx))
    } else {
        None
    };
    RelayDiagnosticsWireSub {
        short_wire_id: short_id(&s.wire_id),
        state_label: title_case(&s.state),
        state_tone: state_tone(&s.state).to_string(),
        consumer_count_label,
        events_rx_display,
        eose_observed: s.eose_at_ms.is_some(),
        opened_ms: event_to_unix_ms(started_unix_ms, s.opened_at_ms).unwrap_or(started_unix_ms),
        last_event_ms: s.last_event_at_ms.and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        eose_ms: s.eose_at_ms.and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        close_reason: s.close_reason,
        wire_id: s.wire_id,
        relay_url: s.relay_url,
        filter_summary: s.filter_summary,
    }
}

// ── Predicates ────────────────────────────────────────────────────────────

fn is_active_state(state: &str) -> bool {
    matches!(state, "open" | "live" | "active" | "opening")
}

#[cfg(test)]
#[path = "relay_diagnostics/tests.rs"]
mod tests;
