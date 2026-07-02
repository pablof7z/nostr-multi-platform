//! `ActionLedgerCommand` — action-stage ledger (host ACK + worker terminal
//! recording).
//!
//! Grouped under `ActorCommand::ActionLedger(ActionLedgerCommand)`. Dispatch
//! home: `actor/dispatch/cmd_publish.rs`.

/// Action-stage ledger verbs: the host/worker → actor seam for the kernel's
/// one `ActionLedger` (the action-stage mirror + per-tick `action_results`
/// drain).
///
/// Each variant folds into [`Kernel::ack_action_stage`] /
/// [`Kernel::record_action_failure`] / [`Kernel::record_action_success`],
/// writing both the `action_stages` mirror (so the host's stage observer sees
/// the terminal) and the `action_results` per-tick drain (so a spinner keyed
/// on the `correlation_id` clears). All three are idempotent w.r.t. a buggy
/// host/worker that re-sends.
#[derive(Debug)]
pub enum ActionLedgerCommand {
    /// Host acknowledgement of a `correlation_id` in the `action_stages`
    /// snapshot mirror. The actor folds the ack into the kernel's one
    /// `ActionLedger`, dropping the entry from the stage-history facet so the
    /// next tick's snapshot no longer carries it. Idempotent: an unknown id
    /// is a silent no-op (D6).
    ///
    /// Originates from the native ack surface. The host calls this after
    /// rendering a terminal stage (`Accepted` or `Failed`) and clearing its UI;
    /// until the ack arrives the entry stays in the
    /// snapshot, so a tick the host missed cannot strand the action's state
    /// machine.
    Ack(String),
    /// Record a terminal `Failed` stage for `correlation_id` on behalf of an
    /// executor that panicked (or otherwise failed *after* the registry
    /// minted the correlation id and before any `ActorCommand` carrying it
    /// could be enqueued).
    ///
    /// Without this seam the failure is orphaned: the host received a
    /// `correlation_id` from `nmp_app_dispatch_action`'s error envelope but
    /// has no way to ACK an `action_stages` entry that was never produced.
    /// The actor folds this command into [`Kernel::record_action_failure`]
    /// — same engine the sign-step failure path uses — so a `Failed`
    /// terminal lands in both `action_stages` (the mirror, for the host's
    /// ACK lifecycle) and `action_results` (the drain, for the host's spinner
    /// cleanup).
    ///
    /// Originates from `nmp_ffi::action::dispatch_action_json` on the FFI
    /// thread when the executor returned an `Err` (including a
    /// `catch_unwind`-converted panic). Idempotent w.r.t. a buggy host that
    /// re-sends — `record_action_failure` records a second `Failed` stage,
    /// which is a benign no-op for the host (it sees the same terminal twice;
    /// the second ACK is a silent no-op).
    RecordFailure {
        correlation_id: String,
        reason: String,
    },
    /// Record a terminal `Accepted` stage for `correlation_id` on behalf of
    /// an off-thread worker whose success outcome is observed outside the
    /// publish engine. The symmetric counterpart to [`Self::RecordFailure`]:
    /// same routing through [`Kernel::record_action_success`], which writes
    /// both the `action_stages` mirror and the `action_results` per-tick drain.
    ///
    /// The motivating consumer is off-band action settlement such as NIP-47
    /// `pay_invoice`: after the kind:23195 wallet response arrives, the runtime
    /// needs to close the original action promise by correlation id. The same
    /// path closes NIP-57 zaps because their LNURL worker dispatches wallet
    /// payment internally instead of asking the host to pay a toasted invoice.
    ///
    /// Idempotent w.r.t. a buggy worker that re-sends —
    /// `record_action_success` records a second `Accepted` stage, which is a
    /// benign no-op for the host.
    RecordSuccess {
        correlation_id: String,
        /// ADR-0071 Decision 4 — opaque structured result body forwarded
        /// verbatim into the `action_results[correlation_id]` row's `result`
        /// field. `nmp-core` NEVER parses it (D0: no protocol noun in the
        /// substrate). `None` for the NWC pay-invoice path; `Some(json)` for a
        /// protocol crate (e.g. a Blossom blob descriptor) carrying a return
        /// payload.
        result_json: Option<String>,
    },
}
