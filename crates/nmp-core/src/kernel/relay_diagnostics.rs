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
//! `last_connected_at` and `last_event_at`, raw connection / auth / role
//! strings) plus a per-wire-subscription enriched row with the same treatment
//! for the detail screen. Shells derive display strings from raw values.
//!
//! Timestamp fields (`last_connected_ms`, `last_event_ms`, `opened_ms`,
//! `eose_ms`) carry Unix epoch milliseconds (u64). Shells format them as
//! "Xs ago" / "Xm ago" etc. at render time via platform helpers
//! (`relativeTimeFromUnixSeconds` on iOS, `formatRelativeTime` on Android).
//! No `format_ago_*` inside projection builders.
//!
//! Emitted under the snapshot `projections` key
//! [`RELAY_DIAGNOSTICS_PROJECTION_KEY`] (`"relay_diagnostics"`). The shell
//! decodes it as a single struct and renders fields directly: no `.filter`,
//! no `.sorted`, no `Date(timeIntervalSince1970:)`.

use serde::Serialize;
use std::collections::BTreeMap;

mod discovery;
mod format;
mod info;
mod notice;
mod reasons;

use super::{Kernel, RelayStatus, WireSubscriptionStatus};
use discovery::discovery_kinds_for_subs;
use format::{auth_tone, connection_tone, interest_state_tone, role_tone, state_tone};
pub(in crate::kernel) use info::RelayDiagnosticsInfo;
pub(in crate::kernel) use notice::RelayDiagnosticsNotice;
use reasons::{build_reasons, RelayConnectionReason};

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
/// "Xs ago" / "Xm ago" at render time. No format_ago_* inside projection
/// builders. Shells derive display strings from raw values.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsRow {
    /// Canonical relay URL — stable list identity. Shells derive short URL
    /// by stripping `ws[s]://` and trailing `/`.
    pub(super) relay_url: String,
    /// Raw role string: `"content"`, `"indexer"`, `"wallet"`, `"outbox"`, etc.
    /// Shells title-case for display.
    pub(super) role: String,
    /// Semantic role hue key — one of `"primary"`, `"write"`, `"accent"`,
    /// `"secondary"`. The shell maps it to a Color enum (UI styling is the
    /// shell's job; the *decision* of which class this row is in lives here).
    pub(super) role_tone: String,
    /// Raw connection string: `"connected"`, `"reconnecting"`,
    /// `"disconnected"`, `"unknown"`, etc. Shells title-case for display.
    pub(super) connection: String,
    /// Semantic connection hue: `"ok" | "warn" | "error" | "muted"`.
    pub(super) connection_tone: String,
    /// Raw auth string: `"ok"`, `"pending"`, `"required"`, `"—"`, etc.
    /// Shells title-case for display (pass-through `"—"` as-is).
    pub(super) auth: String,
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
    /// Reconnect attempts since process start.
    pub(super) reconnect_count: u32,
    /// Raw bytes received counter. Zero when no data yet.
    /// Shells format as "X bytes" / "Y KB" / "Z MB" when > 0.
    pub(super) bytes_rx: u64,
    /// Raw bytes transmitted counter. Zero when no data yet.
    pub(super) bytes_tx: u64,
    /// Unix epoch milliseconds of the last successful connect. `None` when
    /// the relay has never connected. Shells format as "Xs ago" at render time.
    pub(super) last_connected_ms: Option<u64>,
    /// Unix epoch milliseconds of the last event received. `None` when no
    /// events have arrived. Shells format as "Xs ago" at render time.
    pub(super) last_event_ms: Option<u64>,
    /// Most recent NIP-01 NOTICE prose, or `None`.
    pub(super) last_notice: Option<String>,
    /// Total NOTICE frames received (session counter; not capped by the ring).
    pub(in crate::kernel) notice_count: u64,
    /// Bounded NOTICE log, newest first (≤32 entries; wall-clock Unix-ms).
    pub(in crate::kernel) notices: Vec<RelayDiagnosticsNotice>,
    /// Most recent error prose, or `None`.
    pub(super) last_error: Option<String>,
    /// Per-wire-subscription detail rows (newest by sort id last — the
    /// kernel already sorts deterministically by `wire_id`).
    pub(super) wire_subs: Vec<RelayDiagnosticsWireSub>,
    /// Raw discovery kind numbers currently served by open wire subscriptions
    /// on this relay (deduplicated, sorted). Shells format for display;
    /// they do not parse REQ filter JSON.
    pub(super) discovery_kinds: Vec<u64>,
    /// ADR-0051 — the relay's NIP-11 information document, once `nmp-nip11`
    /// has fetched it. `None` until the fetch resolves (or the relay serves
    /// no document). Apps read `info.name` / `info.icon` / … directly — no
    /// HTTP, no JSON, no awareness of NIP-11.
    pub(super) info: Option<RelayDiagnosticsInfo>,
    /// Pre-built connection-reason list derived from `RelayAttribution`. One
    /// entry per routing lane that placed this relay in the plan (NIP-65
    /// outbox, app relay, indexer, hint, …). Empty before the first compile
    /// or when no attribution is available. The `"blocked"` entry is always
    /// first when the relay is in the user's kind:10006 block list.
    pub(crate) reasons: Vec<RelayConnectionReason>,
}

/// Enriched per-subscription view for `WireSubscriptionDetailView` and the
/// list rows on `RelayDetailView`. Timestamp fields carry Unix epoch
/// milliseconds; shells format as "Xs ago" at render time.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayDiagnosticsWireSub {
    /// Full wire id (hex). Stable list identity. Shells derive short id
    /// by taking the first 8 chars + "…" when length > 12.
    pub(super) wire_id: String,
    /// Owning relay URL.
    pub(super) relay_url: String,
    /// Filter prose, propagated unchanged from `WireSub.filter_summary`.
    pub(super) filter_summary: String,
    /// Raw state string, e.g. `"open"`, `"pending"`, `"closed"`.
    /// Shells title-case for display.
    pub(super) state: String,
    /// Semantic state hue: `"ok" | "warn" | "muted" | "error"`.
    pub(super) state_tone: String,
    /// Raw consumer count. Shells format as `"N consumer(s)"` or empty
    /// string when zero.
    pub(super) consumer_count: u32,
    /// Raw events received counter. Shells format as compact count when > 0.
    pub(super) events_rx: u64,
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

        // Snapshot the blocked-relay set and attribution map for reason building.
        let blocked = self.snapshot_blocked_relays();
        let attribution = self.lifecycle.current_plan_attribution();

        let relays: Vec<RelayDiagnosticsRow> = order
            .into_iter()
            .map(|url| {
                let status = by_url.get(&url);
                let subs = subs_by_url.remove(&url).unwrap_or_default();
                let attr = attribution.get(&url);
                let is_blocked = blocked.contains(&url);
                build_relay_row(url, status, subs, started_unix_ms, attr, is_blocked)
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
    attr: Option<&crate::planner::RelayAttribution>,
    is_blocked: bool,
) -> RelayDiagnosticsRow {
    let reasons = build_reasons(attr, is_blocked);
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
            reasons,
            0,
            vec![],
        );
    };
    let info = s.info.as_ref().map(RelayDiagnosticsInfo::from_doc);
    let notice_count = s.notices_rx;
    let notices: Vec<RelayDiagnosticsNotice> = s.notices.iter().rev()
        .map(|n| RelayDiagnosticsNotice { at_ms: n.at_ms, text: n.text.clone() }).collect();
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
        reasons,
        notice_count,
        notices,
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
    reasons: Vec<RelayConnectionReason>,
    notice_count: u64,
    notices: Vec<RelayDiagnosticsNotice>,
) -> RelayDiagnosticsRow {
    let total_sub_count = subs.len() as u32;
    let active_sub_count = subs.iter().filter(|s| is_active_state(&s.state)).count() as u32;
    let eosed_sub_count = subs.iter().filter(|s| s.eose_at_ms.is_some()).count() as u32;
    let total_events_rx = events_rx;
    let discovery_kinds = discovery_kinds_for_subs(&subs);

    let wire_subs = subs
        .into_iter()
        .map(|s| build_wire_sub(s, started_unix_ms))
        .collect();

    RelayDiagnosticsRow {
        relay_url,
        role: role.to_string(),
        role_tone: role_tone(role).to_string(),
        connection: connection.to_string(),
        connection_tone: connection_tone(connection).to_string(),
        auth: auth.to_string(),
        auth_tone: auth_tone(auth).to_string(),
        total_sub_count,
        active_sub_count,
        eosed_sub_count,
        total_events_rx,
        reconnect_count,
        bytes_rx,
        bytes_tx,
        last_connected_ms: last_connected_raw.and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        last_event_ms: last_event_raw.and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        last_notice,
        notice_count,
        notices,
        last_error,
        wire_subs,
        discovery_kinds,
        info,
        reasons,
    }
}

fn build_wire_sub(s: WireSubscriptionStatus, started_unix_ms: u64) -> RelayDiagnosticsWireSub {
    RelayDiagnosticsWireSub {
        state: s.state.clone(),
        state_tone: state_tone(&s.state).to_string(),
        consumer_count: s.logical_consumer_count,
        events_rx: s.events_rx,
        eose_observed: s.eose_at_ms.is_some(),
        opened_ms: event_to_unix_ms(started_unix_ms, s.opened_at_ms).unwrap_or(started_unix_ms),
        last_event_ms: s
            .last_event_at_ms
            .and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
        eose_ms: s
            .eose_at_ms
            .and_then(|ms| event_to_unix_ms(started_unix_ms, ms)),
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
