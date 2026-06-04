//! Engine-internal data types — `InFlight`, `TerminalOutcome`, `LastTerminal`.
//!
//! Extracted from `engine.rs` to keep the orchestrator file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). Pure data + the `LastTerminal`
//! constructor; no engine state lives here, no I/O, no FFI.

use std::collections::BTreeMap;

use super::super::action::{PublishHandle, RelayUrl};
use super::super::state::PerRelayState;
use super::super::traits::RelaySelectionReason;
use crate::substrate::SignedEvent;

/// One in-flight publish row owned by the engine.
pub(crate) struct InFlight {
    pub event: SignedEvent,
    pub per_relay: BTreeMap<RelayUrl, PerRelayState>,
    /// Per-relay selection rationale, captured from `OutboxResolver::resolve()`
    /// at publish time and never mutated thereafter. Mirrors the key set of
    /// `per_relay`; the engine reads it only during projection
    /// (`flush_view` → `EventPublishStatus.relay_reasons`) so the snapshot
    /// projection can render a "why was this relay targeted?" string without
    /// re-running the resolver. The `Vec<RelaySelectionReason>` shape captures
    /// the case where one canonical URL was selected for multiple reasons
    /// (e.g. a relay that is both the author's NIP-65 write relay AND a
    /// discovery indexer). Survives restart via `PublishRecord.relay_reasons`.
    pub relay_reasons: BTreeMap<RelayUrl, Vec<RelaySelectionReason>>,
    pub pending_retries: BTreeMap<RelayUrl, u64>, // relay -> earliest retry epoch ms
    pub dirty: bool,
    /// Optional action `correlation_id` to report in `LastTerminal` instead of
    /// the publish `handle` (== event id). Set when the publish originates
    /// from `nmp_app_dispatch_action`'s `PublishAction::PublishRaw` path: the
    /// actor signs the event, so its `id` is not known at dispatch time and
    /// the host received a registry-minted `correlation_id` that differs from
    /// the event id. The terminal sites (`on_ack`, `tick`) report this id so
    /// the host spinner can be cleared. `None` for every other publish path
    /// (pre-signed `Publish`, `react`, `follow`, …) — the terminal verdict
    /// then uses the `handle`, preserving prior behaviour.
    pub correlation_id_override: Option<String>,
}

/// T128: terminal verdict for a settled publish. The engine records one of
/// these into `recently_completed` the moment `in_flight.remove(handle)` is
/// about to fire (`is_complete == true`), and the kernel drains it via
/// [`super::PublishEngine::take_completed`] to flip the `PublishQueueEntry`
/// status from `accepted_locally` to `"ok"` / `"failed"`.
///
/// `accepted` is the relays that landed `PerRelayState::Ok`; `failed` carries
/// the `(relay_url, reason)` pairs from `FailedAfterRetries`. Mixed publishes
/// (at least one Ok + at least one `FailedAfterRetries`) are reported here with
/// both lists populated — the kernel decides what status string to surface.
///
/// `relay_reasons` carries the per-relay selection rationale captured at
/// publish time (mirrors `InFlight.relay_reasons`). Threaded through so the
/// settled `publish_queue` projection can render the same "why was this
/// relay targeted?" string the in-flight `publish_outbox` projection shows
/// — without that the relay row goes dim the moment the publish completes.
/// Keys mirror the union of `accepted` and `failed` for terminally-settled
/// rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalOutcome {
    pub event_id: String,
    pub accepted: Vec<RelayUrl>,
    pub failed: Vec<(RelayUrl, String)>,
    pub relay_reasons: BTreeMap<RelayUrl, Vec<RelaySelectionReason>>,
}

/// Direction review #29: one terminal action result the engine records into
/// `pending_terminals` so the kernel can drain it into the `action_results`
/// snapshot projection. The host reads `action_results` to clear a per-action
/// spinner — each tick surfaces every action that settled, not just the most
/// recent.
///
/// `correlation_id` is the `PublishHandle` (== `event_id` for publish actions).
/// `status` uses the engine's internal vocabulary `"ok" | "failed" |
/// "cancelled"`; the kernel translates `"ok" → "published"` at the projection
/// serialization site. `error` is `None` for success, otherwise a single
/// human-readable string (the per-relay failure reasons joined with `; `).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastTerminal {
    pub correlation_id: PublishHandle,
    pub status: &'static str,
    pub error: Option<String>,
    /// Opaque structured result body the action carried to a success terminal
    /// (ADR-0043 Decision 4). `nmp-core` NEVER parses this — it is forwarded
    /// verbatim into the `action_results[correlation_id]` row's `result` field
    /// so a protocol crate can attach a descriptor (e.g. a Blossom blob
    /// descriptor) without `nmp-core` learning any protocol noun (D0). `None`
    /// for every publish-engine terminal and the bare `record_action_success`
    /// path; `Some(json)` only on the `RecordActionSuccess { result_json }`
    /// off-band path.
    pub result_json: Option<String>,
}

impl LastTerminal {
    /// Build a `LastTerminal` from a settled `TerminalOutcome`. Mirrors the
    /// kernel's `classify_terminal_outcome` status rule: any accepted relay →
    /// `"ok"`, otherwise `"failed"`.
    ///
    /// `correlation_id_override` is the action `correlation_id` the host received
    /// from `nmp_app_dispatch_action` when it differs from the publish handle
    /// (the `PublishRaw` path — the actor signs the event, so the host got a
    /// registry-minted id, not the event id). When `Some`, the returned
    /// `correlation_id` is that override; when `None`, it falls back to the
    /// `handle` (the pre-existing behaviour for every other publish path).
    pub(super) fn from_outcome(
        handle: &PublishHandle,
        correlation_id_override: Option<&str>,
        outcome: &TerminalOutcome,
    ) -> Self {
        let correlation_id = correlation_id_override.map_or_else(|| handle.clone(), str::to_string);
        if outcome.accepted.is_empty() {
            let error = if outcome.failed.is_empty() {
                Some("publish failed: no relays settled".to_string())
            } else {
                Some(
                    outcome
                        .failed
                        .iter()
                        .map(|(url, reason)| format!("{url}: {reason}"))
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            };
            Self {
                correlation_id,
                status: "failed",
                error,
                result_json: None,
            }
        } else {
            Self {
                correlation_id,
                status: "ok",
                error: None,
                result_json: None,
            }
        }
    }
}
