//! Engine-internal data types — `InFlight`, `TerminalOutcome`, `LastTerminal`.
//!
//! Extracted from `engine.rs` to keep the orchestrator file under the 500-LOC
//! hand-authored ceiling (AGENTS.md / V-12). Pure data + the `LastTerminal`
//! constructor; no engine state lives here, no I/O, no FFI.

use std::collections::BTreeMap;

use super::super::action::{PublishHandle, RelayUrl};
use super::super::state::PerRelayState;
use super::super::traits::RelaySelectionReason;
use nmp_signer_iface::SignedEvent;

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

/// T128: per-relay terminal verdict for a settled publish, carried on the
/// engine→ledger terminal stream ([`LastTerminal::publish_queue`]) so the
/// kernel can refine the event-id-keyed `PublishQueueEntry` from the SAME
/// terminal that records the correlation-id-keyed `action_results` row (S11
/// slice 4, #1758). The engine builds one the moment `in_flight.remove(handle)`
/// is about to fire (`is_complete == true`).
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

/// S11 slice 4 (#1758): how a single engine terminal refines the event-id-keyed
/// `publish_queue` row, carried on [`LastTerminal::publish_queue`] alongside the
/// correlation-id-keyed `action_results` fields. The `publish_queue` row is no
/// longer maintained by a parallel `recently_completed` lane — its terminal
/// status DERIVES from this payload at the moment the kernel folds the terminal
/// into the [`crate::kernel::action_ledger::ActionLedger`]. There is ONE status
/// authority per terminal: the queue status is derived from the `TerminalOutcome`
/// (relay settlement) or is the fixed `"cancelled"` token (user cancel), never a
/// second independent status string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublishQueueTerminal {
    /// A relay-settlement terminal (every relay reached a terminal `PerRelayState`).
    /// The kernel classifies the `TerminalOutcome` into the wire `"ok"` / `"failed"`
    /// status + per-relay outcome map, keyed by `TerminalOutcome::event_id`.
    Settled(TerminalOutcome),
    /// A user-initiated cancel. Flips the queue row keyed by this `event_id`
    /// (the publish handle the kernel resolved the cancel against) to
    /// `"cancelled"` with an empty per-relay outcome map — byte-identical to the
    /// prior explicit `set_publish_entry_terminal(handle, "cancelled", vec![])`.
    /// The id is carried explicitly (non-optional) so the queue update never
    /// unwraps `LastTerminal::event_id` by convention.
    Cancelled { event_id: String },
    /// This terminal does not refine the `publish_queue` (no queue row exists for
    /// it, or one is pushed separately by the kernel error path). The `NoTargets`
    /// failure is the only engine-origin case: `start_publish` returns
    /// `Err(NoTargets)` before any in-flight row, and the kernel error path pushes
    /// the queue row itself.
    None,
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
    /// The nostr `event_id` of the signed event this terminal concerns, when one
    /// exists. For the `PublishRaw` path `correlation_id` is a registry-minted
    /// dispatch id that is NOT the event id, so consumers that need to reference
    /// the just-published event (e.g. a kind:16 group repost building an `e` tag
    /// to it) read this field instead (#1702). Read it together with `status`:
    /// `Some` whenever a signed event backs the terminal (published, failed, or
    /// cancelled); `None` for off-band terminals where no event was ever signed
    /// (sign-step failure, NWC pay-invoice success).
    pub event_id: Option<String>,
    /// Opaque structured result body the action carried to a success terminal
    /// (ADR-0071 Decision 4). `nmp-core` NEVER parses this — it is forwarded
    /// verbatim into the `action_results[correlation_id]` row's `result` field
    /// so a protocol crate can attach a descriptor (e.g. a Blossom blob
    /// descriptor) without `nmp-core` learning any protocol noun (D0). Publish
    /// engine terminals leave this as `None`; the kernel terminal fold attaches
    /// the Rust-owned relay receipt from `PublishQueueTerminal::Settled`.
    /// `Some(json)` is reserved for off-band action successes that already have
    /// their own structured result.
    pub result_json: Option<String>,
    /// Curated kernel policy `reason_code` for shell localization (#1735).
    ///
    /// Set only for kernel-authored refusals (e.g. D10 routing-leak guard)
    /// where the host is expected to show localized copy keyed by the code.
    /// Engine-driven terminals (relay give-up, etc.) leave this `None` —
    /// those carry opaque upstream text that cannot be localized.
    /// `take_action_results_projection` forwards the code into the
    /// `action_lifecycle` projection via `record_action_stage_coded` so
    /// the first coded write is NOT overwritten by a second un-coded pass.
    pub reason_code: Option<&'static str>,
    /// S11 slice 4 (#1758): how this terminal refines the event-id-keyed
    /// `publish_queue` row. The kernel's single terminal fold
    /// (`drain_engine_terminals_into_ledger`) reads this to drive
    /// `set_publish_entry_terminal` from the SAME terminal that records the
    /// `action_results` row — replacing the deleted parallel `recently_completed`
    /// lane. `None` (the enum variant) for off-band / `NoTargets` terminals that
    /// do not touch the queue.
    pub publish_queue: PublishQueueTerminal,
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
                event_id: Some(outcome.event_id.clone()),
                result_json: None,
                reason_code: None,
                // The same settled outcome refines the event-id-keyed queue row.
                publish_queue: PublishQueueTerminal::Settled(outcome.clone()),
            }
        } else {
            Self {
                correlation_id,
                status: "ok",
                error: None,
                event_id: Some(outcome.event_id.clone()),
                result_json: None,
                reason_code: None,
                publish_queue: PublishQueueTerminal::Settled(outcome.clone()),
            }
        }
    }
}
