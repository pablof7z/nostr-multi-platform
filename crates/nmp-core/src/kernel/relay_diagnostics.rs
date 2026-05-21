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
//! cumulative events received, pre-formatted "Xs/m/h ago" labels for
//! `last_connected_at` and `last_event_at`, pre-formatted connection /
//! auth / role labels) plus a per-wire-subscription enriched row with the
//! same treatment for the detail screen.
//!
//! Emitted under the snapshot `projections` key
//! [`RELAY_DIAGNOSTICS_PROJECTION_KEY`] (`"relay_diagnostics"`). The shell
//! decodes it as a single struct and renders fields directly: no `.filter`,
//! no `.sorted`, no `Date(timeIntervalSince1970:)`.

use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

use super::*;

/// Snapshot-projection key under which the diagnostics roll-up is emitted.
/// Keep in sync with the Swift `SnapshotProjections.relayDiagnostics`
/// decoder in `KernelBridge.swift`. The hard-coded key in `update.rs`
/// (`"relay_diagnostics"`) is the wire string; this constant exists to make
/// the choice greppable from the projection module.
#[allow(dead_code)]
pub(super) const RELAY_DIAGNOSTICS_PROJECTION_KEY: &str = "relay_diagnostics";

/// One rolled-up row per known relay URL. Every aggregate (`active_sub_count`,
/// `eosed_sub_count`, `total_events_rx`) is computed here; every display
/// string (`status_label`, `last_connected_display`, `last_event_display`) is
/// pre-formatted here. The shell renders fields directly.
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
    /// Sum of `events_rx` across every wire subscription on this relay.
    pub(super) total_events_rx: u64,
    /// Pre-formatted total events (compact: `"1.2K"`, `"34"`).
    pub(super) total_events_display: String,
    /// Reconnect attempts since process start.
    pub(super) reconnect_count: u32,
    /// Pre-formatted "X bytes" / "Y KB" / "Z MB" label for bytes_rx, or
    /// `None` when the counter is zero.
    pub(super) bytes_rx_display: Option<String>,
    /// Same for bytes_tx.
    pub(super) bytes_tx_display: Option<String>,
    /// Pre-formatted relative time for the last successful connect, e.g.
    /// `"3s ago"`. `None` when the relay never connected.
    pub(super) last_connected_display: Option<String>,
    /// Pre-formatted relative time for the last event received, e.g.
    /// `"42s ago"`. `None` when no events have arrived.
    pub(super) last_event_display: Option<String>,
    /// Most recent NIP-01 NOTICE prose, or `None`.
    pub(super) last_notice: Option<String>,
    /// Most recent error prose, or `None`.
    pub(super) last_error: Option<String>,
    /// Per-wire-subscription detail rows (newest by sort id last — the
    /// kernel already sorts deterministically by `wire_id`).
    pub(super) wire_subs: Vec<RelayDiagnosticsWireSub>,
}

/// Enriched per-subscription view for `WireSubscriptionDetailView` and the
/// list rows on `RelayDetailView`. Every display field is pre-formatted.
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
    /// Pre-formatted relative time the sub opened.
    pub(super) opened_display: String,
    /// Pre-formatted relative time for the last event, or `None`.
    pub(super) last_event_display: Option<String>,
    /// Pre-formatted relative time for EOSE, or `None`.
    pub(super) eose_display: Option<String>,
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
    /// then outbox-only URLs in BTreeSet (lexicographic) order. The shell
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
        // "Now since kernel start" in ms — the same time axis the kernel's
        // `elapsed_ms()` returns. We use it to compute "X ago" labels
        // without ever leaving ms-since-start space (avoiding the
        // `Instant`-as-`UNIX_EPOCH` confusion that the Swift shell had).
        let now_ms = self.now_since_start_ms();

        // Pre-compute statuses keyed by relay URL so each row can be filled
        // without a per-row linear scan back through `relay_statuses`.
        let statuses = self.relay_statuses();
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
                build_relay_row(url, status, subs, now_ms)
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

    /// Milliseconds since `started_at`. Returns 0 if the kernel never
    /// started (so the formatter degrades to `"now"` rather than panicking
    /// across the FFI boundary, D6).
    fn now_since_start_ms(&self) -> u128 {
        match self.timing.started_at {
            Some(started) => Instant::now().duration_since(started).as_millis(),
            None => 0,
        }
    }
}

fn build_relay_row(
    relay_url: String,
    status: Option<&RelayStatus>,
    subs: Vec<WireSubscriptionStatus>,
    now_ms: u128,
) -> RelayDiagnosticsRow {
    let (role, connection, auth, reconnect_count, last_connected, last_event, last_notice,
        last_error, bytes_rx, bytes_tx) = match status {
        Some(s) => (
            s.role.as_str(),
            s.connection.as_str(),
            s.auth.as_str(),
            s.reconnect_count,
            s.last_connected_at_ms,
            s.last_event_at_ms,
            s.last_notice.clone(),
            s.last_error.clone(),
            s.bytes_rx,
            s.bytes_tx,
        ),
        // Synthetic row for an outbox-only URL with no `RelayStatus` lane —
        // mirrors the old Swift `syntheticRelayStatus` helper but stays Rust-
        // owned so the shell renders fields directly.
        None => {
            let active_count = subs
                .iter()
                .filter(|s| is_active_state(&s.state))
                .count();
            let connection = if active_count > 0 {
                "connected"
            } else {
                "unknown"
            };
            let last_event = subs.iter().filter_map(|s| s.last_event_at_ms).max();
            return finish_row(relay_url, "outbox", connection, "—", 0, None, last_event,
                None, None, 0, 0, subs, now_ms);
        }
    };
    finish_row(
        relay_url,
        role,
        connection,
        auth,
        reconnect_count,
        last_connected,
        last_event,
        last_notice,
        last_error,
        bytes_rx,
        bytes_tx,
        subs,
        now_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_row(
    relay_url: String,
    role: &str,
    connection: &str,
    auth: &str,
    reconnect_count: u32,
    last_connected: Option<u128>,
    last_event: Option<u128>,
    last_notice: Option<String>,
    last_error: Option<String>,
    bytes_rx: u64,
    bytes_tx: u64,
    subs: Vec<WireSubscriptionStatus>,
    now_ms: u128,
) -> RelayDiagnosticsRow {
    let total_sub_count = subs.len() as u32;
    let active_sub_count = subs
        .iter()
        .filter(|s| is_active_state(&s.state))
        .count() as u32;
    let eosed_sub_count = subs.iter().filter(|s| s.eose_at_ms.is_some()).count() as u32;
    let total_events_rx: u64 = subs.iter().map(|s| s.events_rx).sum();

    let wire_subs = subs
        .into_iter()
        .map(|s| build_wire_sub(s, now_ms))
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
        last_connected_display: last_connected.map(|ms| format_ago_ms(now_ms, ms)),
        last_event_display: last_event.map(|ms| format_ago_ms(now_ms, ms)),
        last_notice,
        last_error,
        wire_subs,
    }
}

fn build_wire_sub(s: WireSubscriptionStatus, now_ms: u128) -> RelayDiagnosticsWireSub {
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
        opened_display: format_ago_ms(now_ms, s.opened_at_ms),
        last_event_display: s.last_event_at_ms.map(|ms| format_ago_ms(now_ms, ms)),
        eose_display: s.eose_at_ms.map(|ms| format_ago_ms(now_ms, ms)),
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

// ── Hue selectors (semantic tone, not a Color value) ─────────────────────

fn role_tone(role: &str) -> &'static str {
    match role {
        "write" => "write",
        "read" => "accent",
        _ => "accent",
    }
}

fn connection_tone(connection: &str) -> &'static str {
    let lower = connection.to_ascii_lowercase();
    if lower == "connected" {
        "ok"
    } else if lower.starts_with("disconnect") || lower == "failed" {
        "error"
    } else if lower.contains("connect") {
        // "reconnecting", "connecting", "auth_paused_will_reconnect", etc.
        "warn"
    } else if lower == "unknown" || lower == "idle" || lower == "—" {
        "muted"
    } else {
        "error"
    }
}

fn auth_tone(auth: &str) -> &'static str {
    let lower = auth.to_ascii_lowercase();
    if lower == "ok" || lower == "authenticated" {
        "ok"
    } else if lower == "pending" {
        "warn"
    } else {
        "muted"
    }
}

fn state_tone(state: &str) -> &'static str {
    match state.to_ascii_lowercase().as_str() {
        "open" | "active" | "live" => "ok",
        "pending" | "warming" | "opening" | "auth_paused" => "warn",
        "closed" | "done" | "closed_by_relay" => "muted",
        _ => "muted",
    }
}

fn interest_state_tone(state: &str) -> &'static str {
    match state {
        "active" | "warming" | "tailing" | "complete" => "ok",
        "idle" => "muted",
        "opening" | "queued" | "loading" | "backfilling" => "warn",
        _ => "warn",
    }
}

// ── String formatters ────────────────────────────────────────────────────

fn role_label(role: &str) -> String {
    if role.is_empty() {
        "—".to_string()
    } else {
        title_case(role)
    }
}

fn auth_label(auth: &str) -> String {
    if auth == "—" {
        auth.to_string()
    } else {
        title_case(auth)
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    for c in s.chars() {
        if first {
            for u in c.to_uppercase() {
                out.push(u);
            }
            first = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn short_relay_url(url: &str) -> String {
    let stripped = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url);
    stripped.trim_end_matches('/').to_string()
}

fn short_id(id: &str) -> String {
    if id.chars().count() <= 12 {
        id.to_string()
    } else {
        let head: String = id.chars().take(8).collect();
        format!("{head}…")
    }
}

fn format_bytes(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    if kb < 1.0 {
        format!("{} B", bytes)
    } else if kb < 1024.0 {
        format!("{:.1} KB", kb)
    } else {
        format!("{:.1} MB", kb / 1024.0)
    }
}

fn compact_count(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        if v.fract() == 0.0 {
            format!("{}K", v as u64)
        } else {
            format!("{:.1}K", v)
        }
    } else if n < 1_000_000_000 {
        let v = n as f64 / 1_000_000.0;
        if v.fract() == 0.0 {
            format!("{}M", v as u64)
        } else {
            format!("{:.1}M", v)
        }
    } else {
        let v = n as f64 / 1_000_000_000.0;
        format!("{:.1}B", v)
    }
}

/// Format an "X ago" label given a `now` and a `then`, both in
/// milliseconds-since-kernel-start. Returns `"now"` when `then >= now`
/// (clock skew or 0-elapsed kernels).
fn format_ago_ms(now_ms: u128, then_ms: u128) -> String {
    if then_ms == 0 || now_ms <= then_ms {
        return "now".to_string();
    }
    let diff_ms = now_ms - then_ms;
    let secs = diff_ms / 1_000;
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ago_buckets() {
        assert_eq!(format_ago_ms(10_000, 9_500), "0s ago");
        assert_eq!(format_ago_ms(60_000, 0), "now"); // then==0 means never observed
        assert_eq!(format_ago_ms(120_000, 60_000), "1m ago");
        assert_eq!(format_ago_ms(3_700_000, 100_000), "1h ago");
        assert_eq!(format_ago_ms(90_000_000, 0_001), "1d ago");
    }

    #[test]
    fn compact_count_buckets() {
        assert_eq!(compact_count(0), "0");
        assert_eq!(compact_count(42), "42");
        assert_eq!(compact_count(999), "999");
        assert_eq!(compact_count(1_000), "1K");
        assert_eq!(compact_count(1_234), "1.2K");
        assert_eq!(compact_count(1_000_000), "1M");
        assert_eq!(compact_count(2_500_000), "2.5M");
    }

    #[test]
    fn short_relay_strips_scheme_and_trailing_slash() {
        assert_eq!(short_relay_url("wss://relay.example/"), "relay.example");
        assert_eq!(short_relay_url("ws://relay.example/path"), "relay.example/path");
        assert_eq!(short_relay_url("relay.example"), "relay.example");
    }

    #[test]
    fn connection_tone_classifies_states() {
        assert_eq!(connection_tone("connected"), "ok");
        assert_eq!(connection_tone("Reconnecting"), "warn");
        assert_eq!(connection_tone("Disconnected"), "error");
        assert_eq!(connection_tone("unknown"), "muted");
    }

    #[test]
    fn snapshot_emits_one_row_per_known_relay() {
        use crate::relay::DEFAULT_VISIBLE_LIMIT;
        let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let snap = kernel.relay_diagnostics_snapshot();
        // Bootstrap roles (Content + Indexer) are always present.
        let roles: Vec<_> = snap.relays.iter().map(|r| r.role_label.as_str()).collect();
        assert!(
            roles.iter().any(|r| *r == "Content"),
            "expected Content lane in roles {:?}",
            roles
        );
        assert!(
            roles.iter().any(|r| *r == "Indexer"),
            "expected Indexer lane in roles {:?}",
            roles
        );
        // Every relay row has roll-up counters zeroed (no subs yet).
        for row in &snap.relays {
            assert_eq!(row.total_sub_count, 0);
            assert_eq!(row.active_sub_count, 0);
            assert_eq!(row.eosed_sub_count, 0);
            assert_eq!(row.total_events_rx, 0);
            assert_eq!(row.total_events_display, "0");
        }
        // The interest snapshot includes the always-on lanes.
        assert!(snap.interests.iter().any(|i| i.key == "Timeline"));
        // Every interest carries a non-empty semantic tone.
        for interest in &snap.interests {
            assert!(!interest.state_tone.is_empty());
        }
    }
}
