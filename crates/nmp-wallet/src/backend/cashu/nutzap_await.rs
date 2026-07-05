//! #3010 (second requirement) — await the recipient's kind:10019 instead of
//! failing `nutzap.send` closed on first cache miss.
//!
//! # The gap this closes
//!
//! Before this, `send.rs`'s `SendNutzapCommand::run` reacted to a kind:10019
//! cache miss by opening a read interest (`ctx.ensure_interest`, warming the
//! cache for NEXT time) and then STILL failing this attempt closed
//! (`NO_RECIPIENT_NUTZAP_INFO`) — the caller's own retry loop was the only
//! thing that ever picked the recipient's info back up once it arrived. The
//! app just wants to say "pay" once; this module makes the miss self-driving
//! instead: park the send, and the moment the recipient's kind:10019 lands,
//! redrive it automatically.
//!
//! # The event-arrival seam (D8 — no polling)
//!
//! Mirrors `nmp-marmot`'s `MarmotIngestParser` (the peer-KeyPackage /
//! gift-wrap ingest path) — the established "register a continuation, redrive
//! on event arrival" primitive already proven in this codebase — via the same
//! underlying kernel primitive Marmot itself uses:
//! `nmp_core::substrate::IngestParser`, registered through
//! `IngestParserRegistrar::replace_ingest_parser`. The kernel's
//! `EventIngestDispatcher` fans every accepted kind:10019 event, from ANY
//! interest that caused the kernel to accept it (including `send.rs`'s
//! per-recipient `ctx.ensure_interest` call), to every registered parser for
//! that kind. [`NutzapInfoArrivalParser`] is that parser: on every kind:10019
//! this wallet's kernel ingests, it looks up
//! `CashuWalletState::pending_sends` for that event's author and, if any
//! `SendNutzap`s are parked on it, redrives each one (fresh journal
//! operation, same caller correlation id) via the captured
//! [`CommandSender`] — never touching the actor's command queue from inside
//! the ingest call itself in a way that could re-enter (the command is merely
//! enqueued, run on a later actor tick, same as every other cross-thread
//! `ActorCommand::Protocol` dispatch in this crate).
//!
//! # The bound (D8 — wall-clock-gated, not a busy loop)
//!
//! [`NutzapAwaitTtlSweep`] implements `RelayTextInterceptor::on_idle_tick` —
//! the actor's existing ~250ms idle-loop hook (already used by
//! `nmp-nip47`'s `WalletInterceptor` for its own pending-payment TTL sweep;
//! see that type's doc comment). It never inspects relay text (`on_
//! relay_text` is a permanent no-op here); it only compares
//! `kernel.now_secs()` against each parked await's `parked_at_secs` and fails
//! closed (`NO_RECIPIENT_NUTZAP_INFO`) anything older than
//! [`NUTZAP_INFO_AWAIT_TIMEOUT_SECS`] — this is what guarantees a genuinely
//! absent recipient still terminates even if no further kind:10019 (for ANY
//! recipient) ever arrives to trigger an opportunistic sweep.
//!
//! # At-most-once
//!
//! Each `PendingSendAwait` is removed from `pending_sends`
//! ([`CashuWalletState::take_send_awaits`] /
//! [`CashuWalletState::sweep_expired_send_awaits`]) the instant it is either
//! redriven or swept as expired — it can never fire twice, and the original
//! (superseded) send attempt never itself proceeds (see `send.rs`'s miss
//! branch: the original operation is transitioned `Failed` at park time,
//! before this module ever sees it).

use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    build_record_action_failure, IngestParser, IngestParserRegistrar, RelayTextInterceptor,
    RelayTextInterceptorRegistrar,
};
use nmp_core::ui_token::UiToken;
use nmp_core::{CommandSender, Kernel, OutboundMessage};
use nmp_store::VerifiedEvent;

use crate::journal::{WalletOperationId, WalletOperationKind};

use super::send::SendNutzapCommand;
use super::state::{lock_state, CashuWalletState, PendingSendAwait};
use super::{ui_codes, CashuWalletBackend};

/// Slot key this module's `IngestParser` registration owns — globally unique
/// across crates (see `IngestParserRegistrar::replace_ingest_parser`'s doc
/// comment on why that matters).
const NUTZAP_AWAIT_INGEST_SLOT: &str = "nmp.wallet.nutzap_info_await";

/// How long a `SendNutzap` waits for the recipient's kind:10019 before
/// failing closed `NO_RECIPIENT_NUTZAP_INFO` — long enough to cover a
/// realistic relay round trip (REQ -> EOSE) for a cold `ensure_interest`
/// lookup, short enough that a genuinely-absent recipient doesn't leave a
/// caller's spinner hanging indefinitely.
pub(super) const NUTZAP_INFO_AWAIT_TIMEOUT_SECS: u64 = 20;

impl CashuWalletBackend {
    /// Install the kind:10019-arrival ingest parser + TTL-sweep interceptor
    /// (#3010) — called once from `crate::register::register` (the
    /// composition root), alongside this backend's other wiring. `tx` is the
    /// same actor command sender every other Cashu worker uses.
    pub(crate) fn install_nutzap_await(
        &self,
        tx: CommandSender,
        app: &(impl IngestParserRegistrar + RelayTextInterceptorRegistrar),
    ) {
        let state = Arc::clone(&self.state);
        app.replace_ingest_parser(
            nmp_nip60::kinds::KIND_NIP61_NUTZAP_INFO,
            NUTZAP_AWAIT_INGEST_SLOT,
            Arc::new(NutzapInfoArrivalParser {
                state: Arc::clone(&state),
                tx: tx.clone(),
            }),
        );
        app.add_relay_text_interceptor(Arc::new(NutzapAwaitTtlSweep { state, tx }));
    }
}

/// The `IngestParser` half — see module docs. `pub(super)` (not private) so
/// `send_nutzap_await_tests.rs` (an integration test living under
/// `backend::cashu::tests`, a sibling tree — not a descendant of this
/// module) can construct one directly and simulate a kind:10019 arrival
/// through the REAL backend without needing a live kernel/relay.
pub(super) struct NutzapInfoArrivalParser {
    pub(super) state: Arc<Mutex<CashuWalletState>>,
    pub(super) tx: CommandSender,
}

impl IngestParser for NutzapInfoArrivalParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.parse_at(evt, 0);
    }

    fn parse_at(&self, evt: &VerifiedEvent, now_secs: u64) {
        let author = evt.raw().pubkey.clone();
        let awaits = lock_state(&self.state).take_send_awaits(&author);
        for parked in awaits {
            redrive(&self.state, &self.tx, parked, now_secs);
        }
    }
}

/// Redrive one parked `SendNutzap` under a FRESH journal operation id (never
/// the original — that operation is already terminal `Failed`, superseded at
/// park time), carrying the SAME caller `correlation_id` through so the
/// caller's one-shot action-result channel resolves on this fresh attempt.
/// Mirrors `cross_mint_publish::dispatch_cross_mint_token_event`'s identical
/// "fresh op id, same correlation id" retry shape.
fn redrive(
    state: &Arc<Mutex<CashuWalletState>>,
    tx: &CommandSender,
    parked: PendingSendAwait,
    now_secs: u64,
) {
    let retry_op = WalletOperationId::new(format!("nutzap-await-redrive-{}", parked.await_id));
    let began = {
        let mut guard = lock_state(state);
        let ok = guard
            .begin_operation_at(retry_op.clone(), WalletOperationKind::SendNutzap, now_secs)
            .is_ok();
        if ok {
            let _ = guard.journal.record_amount(&retry_op, parked.amount_sats);
        }
        ok
    };
    if !began {
        // Unreachable in practice (`await_id` is a fresh monotonic counter,
        // so `retry_op` can never collide) — fail closed rather than drop
        // the caller's correlation id silently (D6).
        if let Some(id) = parked.correlation_id {
            let _ = tx.send(build_record_action_failure(
                id,
                "recipient's kind:10019 arrived, but the redrive's journal operation could not \
                 begin — retry nutzap.send"
                    .to_string(),
            ));
        }
        return;
    }
    let _ = tx.send(ActorCommand::Protocol(Box::new(SendNutzapCommand {
        state: Arc::clone(state),
        operation_id: retry_op,
        account_pubkey: parked.account_pubkey,
        recipient_pubkey: parked.recipient_pubkey,
        amount_sats: parked.amount_sats,
        target_event_id: parked.target_event_id,
        correlation_id: parked.correlation_id,
    })));
}

/// The bounded-TTL half — see module docs. Never intercepts relay text
/// (`on_relay_text` is a permanent no-op); only `on_idle_tick` does anything,
/// and it never touches `kernel` beyond reading the wall clock.
struct NutzapAwaitTtlSweep {
    state: Arc<Mutex<CashuWalletState>>,
    tx: CommandSender,
}

impl RelayTextInterceptor for NutzapAwaitTtlSweep {
    fn on_relay_text(
        &self,
        _kernel: &mut Kernel,
        _relay_url: &str,
        _text: &str,
    ) -> Vec<OutboundMessage> {
        Vec::new()
    }

    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        run_ttl_sweep(&self.state, &self.tx, kernel.now_secs());
        Vec::new()
    }
}

/// The TTL sweep's actual logic, factored out `Kernel`-free so it is directly
/// unit-testable (`on_idle_tick`'s only job is supplying `kernel.now_secs()`
/// — mirrors `nmp-nip47`'s `WalletInterceptor::on_idle_tick` delegating to
/// `sweep_expired_payments`/`tick_heartbeat`). Fails every expired parked
/// await closed (`NO_RECIPIENT_NUTZAP_INFO`) — a TTL elapsing with no
/// kind:10019 ever arriving genuinely does mean "this recipient has no
/// reachable nutzap info", unlike (for contrast) `nmp-nip47`'s own payment
/// TTL sweep, which deliberately does NOT treat its elapsed TTL as a definite
/// failure.
pub(super) fn run_ttl_sweep(state: &Mutex<CashuWalletState>, tx: &CommandSender, now_secs: u64) {
    let expired =
        lock_state(state).sweep_expired_send_awaits(now_secs, NUTZAP_INFO_AWAIT_TIMEOUT_SECS);
    for parked in expired {
        let Some(id) = parked.correlation_id else {
            continue;
        };
        let reason =
            "recipient's kind:10019 nutzap info never arrived within the wait bound".to_string();
        let token = UiToken::error(ui_codes::NO_RECIPIENT_NUTZAP_INFO, reason.clone());
        let _ = tx.send(ActorCommand::ShowErrorToken { token });
        let _ = tx.send(build_record_action_failure(id, reason));
    }
}

#[cfg(test)]
#[path = "tests/nutzap_await_tests.rs"]
mod tests;
