//! Public value types for the push-model [`crate::pool::Pool`] API
//! (`docs/architecture/crate-boundaries.md` §3.8 — step 8 phase B).
//!
//! These types are the wire-layer vocabulary the kernel actor speaks to
//! `nmp-network`. They are deliberately substrate-grade: no protocol nouns
//! (no kind, no pubkey, no sub-id), only frame-shape and lifecycle.

use std::time::Duration;

use crate::role::RelayRole;

/// Stringly-typed relay URL. Matches `nmp_core::relay::RelayUrl`
/// (`pub type RelayUrl = String`) so handing a URL across the
/// `nmp-network` / `nmp-core` boundary is a no-op.
///
/// The pool canonicalizes input URLs internally (whitespace trim +
/// lowercase scheme/host); two URLs that canonicalize to the same string
/// share one [`super::RelayHandle`].
pub type RelayUrl = String;

/// Generational handle identifying one (URL, open-count) pair inside the
/// pool. The spec calls for **structural rejection** of stale handles:
/// after a reconnect, the prior generation's handle is no longer valid
/// for `send`/`health`/`close`, and the pool's translator-thread drops
/// any events tagged with a generation that no longer matches the
/// current entry.
///
/// `RelayHandle` is `Copy`-cheap; the kernel actor can store many of
/// them in `wire_subs` without thinking about lifetimes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RelayHandle {
    /// Pool-assigned dense id for the URL slot. Reused after [`super::Pool::close`].
    pub(crate) slot: u32,
    /// Monotonically increasing per-slot generation. Bumped on every
    /// fresh `ensure_open` that re-opens a previously closed slot.
    pub(crate) generation: u64,
}

impl RelayHandle {
    /// Dense slot id (URL identity). Two handles for the same URL across
    /// reconnects share a slot but differ in [`Self::generation`].
    #[must_use]
    pub fn slot(self) -> u32 {
        self.slot
    }

    /// Per-slot generation. Stale handles (from before a reconnect)
    /// carry an older generation and are structurally rejected by the
    /// pool's `send`/`health`/`close`.
    #[must_use]
    pub fn generation(self) -> u64 {
        self.generation
    }
}

/// One outbound frame to push at a specific [`RelayHandle`].
///
/// The Pool API is **push-model**: callers do not enumerate connected
/// relays and there is no "send to all" method. The kernel actor
/// iterates its `RoutedRelaySet` itself and issues one
/// `pool.send(handle, frame)` per URL — the structural answer to NDK
/// issue #175.
#[derive(Clone, Debug)]
pub enum WireFrame {
    /// UTF-8 text payload (NIP-01 JSON: `["REQ", ...]`, `["EVENT", ...]`,
    /// `["CLOSE", ...]`, `["AUTH", ...]`).
    Text(String),
    /// Opaque binary payload. Reserved for future binary NIPs; today no
    /// caller emits this variant.
    #[allow(dead_code)]
    Binary(Vec<u8>),
}

/// One inbound WebSocket frame surfaced by the pool to the kernel.
///
/// Mirrors `nmp_core::kernel::relay_frame::RelayFrame` shape so the
/// `nmp-network → nmp-core` adapter at the actor seam is a 1:1
/// variant-rename. Defined here (not imported from `nmp-core`) because
/// `nmp-network` MUST NOT depend on `nmp-core` — that direction would
/// re-introduce the cycle the step-8 extraction broke.
///
/// ## Step 8 phase E — `Auth` variant
///
/// Per `docs/architecture/crate-boundaries.md` §3.8: the wire layer
/// pre-classifies the inbound `["AUTH", <challenge>]` frame into the
/// [`RelayFrame::Auth`] variant so the kernel sees a typed signal
/// rather than re-discovering AUTH from raw text on the fast path.
/// **This is the only AUTH-aware behaviour in `nmp-network`.** The
/// crate still does NOT:
///
/// - construct the kind:22242 event (lives in `nmp-nip42::builder`),
/// - own the per-relay handshake driver (lives in `nmp-nip42::flow`
///   and the kernel's `kernel/auth.rs` mirror),
/// - pause/replay subscriptions on a challenge (lives in
///   `nmp-core::subs::AuthGate`).
///
/// Text frames that aren't AUTH (or are malformed AUTH — empty
/// challenge, wrong shape) still surface as [`RelayFrame::Text`]; the
/// kernel's ingest parser handles those uniformly.
#[derive(Debug)]
pub enum RelayFrame {
    /// Text payload — every NIP-01 frame other than AUTH the wire layer
    /// can pre-classify (`EVENT` / `EOSE` / `OK` / `NOTICE` / `CLOSED`
    /// are intentionally not pre-parsed here; the kernel ingest path
    /// already owns that parse and the wire layer must not duplicate
    /// the semantic vocabulary).
    Text(String),
    /// Pre-classified `["AUTH", <challenge>]` frame. The wire layer
    /// only extracts the non-empty challenge string (NIP-42 wire
    /// shape); it does not compute the kind:22242 response nor decide
    /// whether to pause subscriptions — those are the kernel's
    /// `kernel/auth.rs` and `subs::AuthGate` jobs respectively.
    ///
    /// Malformed AUTH frames (empty challenge, missing fields) fall
    /// through to [`Self::Text`] so the kernel can log them via the
    /// existing parse path.
    Auth(String),
    /// Binary payload — counted but otherwise ignored by the kernel.
    Binary(Vec<u8>),
    /// Keepalive ping (server → client). The pool surfaces this for
    /// diagnostic counters; the keepalive FSM consumes its own inbound
    /// signal internally.
    Ping,
    /// Keepalive pong (server → client). The native worker drops pongs
    /// before they reach this variant today (the keepalive FSM only
    /// needs the inbound-silence reset); kept for symmetry with the
    /// `nmp-core::RelayFrame` shape.
    Pong,
    /// Connection close — optional reason string for `last_error`
    /// surfacing.
    Close(Option<String>),
}

/// Why a [`super::PoolEvent::Closed`] was emitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosedReason {
    /// `Pool::close(handle)` was called.
    Requested,
    /// `Pool::shutdown()` was called and tore down every worker.
    Shutdown,
    /// The relay closed the socket; the pool will not auto-reconnect.
    /// Today this maps to `RelayWorkerResult::PermanentFailure`
    /// (HTTP 401/403/Forbidden mid-session).
    Permanent,
}

/// Transport-layer error surfaced on [`super::PoolEvent::Failed`].
///
/// Substrate-grade: a string-typed envelope so the kernel doesn't have
/// to pattern-match `tungstenite::Error` (which lives in this crate's
/// native feature only). The kernel logs the message and decides
/// per-URL bookkeeping; the pool decides reconnect-or-not.
#[derive(Clone, Debug)]
pub struct TransportError {
    /// Human-readable error (e.g. `"connection reset by peer"`,
    /// `"403 Forbidden"`). The kernel actor splats this into
    /// `RelayStatus.last_error`.
    pub message: String,
    /// True when the relay denied the client permanently
    /// (HTTP 401/403). Mirrors
    /// [`crate::relay_protocol::is_permanent_error`]. The pool stops
    /// reconnecting on permanent errors.
    pub permanent: bool,
}

/// Per-handle health snapshot.
///
/// This is the **transport-layer** health view (latency, error counts,
/// connection lifecycle), distinct from `nmp_core::kernel::types::RelayHealth`
/// which is kernel-internal state for the diagnostic projection. The
/// kernel actor reads this on demand via [`super::Pool::health`] (or
/// receives it pushed on a [`super::PoolEvent::Health`]).
///
/// Phase B keeps this minimal — V-13's per-relay latency histogram and
/// the NIP-11 capability map are deferred to phases C/D where the
/// signer-broker migration motivates the wider surface.
#[derive(Clone, Debug, Default)]
pub struct RelayHealth {
    /// Most recently observed connection state.
    pub state: HealthState,
    /// Count of successful `Connected` transitions (≥1 once the URL has
    /// ever opened).
    pub connect_count: u64,
    /// Count of `Failed` events emitted for this slot since
    /// `ensure_open`.
    pub failure_count: u64,
    /// Last failure message, if any. Cleared on a fresh `Connected`.
    pub last_error: Option<String>,
    /// Approximate round-trip on the most recent successful keepalive
    /// ping. `None` until the first pong arrives.
    pub last_ping_rtt: Option<Duration>,
}

/// Coarse-grained connection state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HealthState {
    /// The slot has been allocated but no `ensure_open` has resolved yet.
    #[default]
    Idle,
    /// Worker is dialing; no `Connected` yet.
    Connecting,
    /// Socket open; sending and receiving frames.
    Connected,
    /// Worker is reconnecting after a mid-session drop.
    Reconnecting,
    /// Permanently closed (HTTP 401/403 or `Pool::close`).
    Closed,
}

/// Pool configuration knobs.
///
/// Phase B ships the substrate; the storm-protection knobs
/// (`per_relay_reconnect_rate`, `socket_budget`, NIP-11 capability hook)
/// land in phases C/D when the wasm driver and signer-broker migration
/// motivate them. Defaults preserve today's `relay_worker` behaviour
/// bit-for-bit.
#[derive(Clone, Debug)]
pub struct PoolConfig {
    /// Default diagnostic lane to tag workers spawned by
    /// [`super::Pool::ensure_open`]. Today's actor passes the per-URL
    /// role through [`super::Pool::ensure_open_with_role`]; the
    /// no-role overload uses this default.
    pub default_role: RelayRole,
    /// Optional override for the keepalive idle threshold; tests on
    /// millisecond budgets pass a small value. `None` → production
    /// constant `KEEPALIVE_IDLE_THRESHOLD` (30 s).
    pub keepalive_idle: Option<Duration>,
    /// Optional override for the keepalive pong timeout. `None` →
    /// production constant `KEEPALIVE_PONG_TIMEOUT` (30 s).
    pub keepalive_pong_timeout: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            default_role: RelayRole::Content,
            keepalive_idle: None,
            keepalive_pong_timeout: None,
        }
    }
}

/// One row in a [`PoolSnapshot`].
#[derive(Clone, Debug)]
pub struct PoolSnapshotRow {
    pub handle: RelayHandle,
    pub url: RelayUrl,
    pub role: RelayRole,
    pub health: RelayHealth,
}

/// Diagnostic snapshot of every active slot in the pool. Cheap-to-take
/// (clones the rows under the inner lock); used by the FFI status
/// projection.
#[derive(Clone, Debug, Default)]
pub struct PoolSnapshot {
    pub rows: Vec<PoolSnapshotRow>,
}

/// Push-model event channel item. The kernel actor `recv`s these on
/// the `events: Sender<PoolEvent>` it handed to [`super::Pool::new`].
///
/// Stale events (whose `generation` no longer matches the current
/// `slot.generation`) are dropped by the translator thread inside the
/// pool — the kernel never observes a `Frame { generation: stale, .. }`
/// for a URL it has since reconnected. This is the runtime half of the
/// structural-handle-rejection invariant.
#[derive(Debug)]
pub enum PoolEvent {
    Opened {
        h: RelayHandle,
        url: RelayUrl,
        generation: u64,
    },
    Frame {
        h: RelayHandle,
        generation: u64,
        frame: RelayFrame,
    },
    Closed {
        h: RelayHandle,
        generation: u64,
        reason: ClosedReason,
    },
    Failed {
        h: RelayHandle,
        generation: u64,
        error: TransportError,
    },
    Health {
        h: RelayHandle,
        snapshot: RelayHealth,
    },
}
