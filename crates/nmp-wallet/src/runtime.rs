//! W6/W7 (#2908, epic #2864) — the wallet's runtime controller.
//!
//! [`WalletRuntime`] wires two identity-change-reactive observed projections
//! (`nutzap_receipts_shape`/`wallet_self_authored_shape`, see `interests.rs`)
//! and routes every observed `KernelEvent` to each registered backend's
//! [`WalletBackend::on_wallet_event`], forwarding whatever `ActorCommand`s
//! come back onto the actor's command channel.
//!
//! # Identity-change-driven, not tick-polled
//!
//! Every interest this runtime needs is account-scoped in the sense that it
//! is re-derived from the active pubkey on every account change (`#p`=self or
//! `authors`=self), but [`Self::new`] runs during the app's *config* phase —
//! before the kernel starts, so before any account is known. This mirrors
//! `nmp-nip51::register_mute_runtime`'s recipe exactly: an
//! [`nmp_core::substrate::ObservedProjectionReconciler`] per interest shape,
//! driven by [`nmp_core::substrate::IdentityChangeRegistrar`] (fires only on
//! an actual account change — sign-in, switch, or logout — never on ordinary
//! snapshot ticks) plus one eager `sync()` call at registration time to cover
//! cold start (the account may already be active before this call runs).
//! `nmp-wot`'s older tick-per-snapshot polling predates this reconciler and
//! is not the pattern to follow for new code.
//!
//! # `nutzap_receipts` opens `Global`, not `ActiveAccount` (#2942)
//!
//! The reconciler's `scope` argument is NOT a per-account privacy boundary —
//! `InterestScope::ActiveAccount` and `::Global` both key into the exact same
//! registry `SubScope::Global` slot space
//! (`nmp-core/src/subs/interest_builder.rs`), and account-switch handling is
//! entirely this module's own job (the `sync()` calls below, driven by the
//! identity-change observer). `scope` instead selects which relay-routing
//! lanes the planner's compiler (`nmp-planner/src/compiler/partition/mod.rs`)
//! is willing to fire for the interest. `nutzap_receipts_shape` is a `#p`=self
//! filter with `PTagRouting::Nip65ReadRelays` (the default): the compiler's
//! Case C routes it to the tagged pubkey's OWN cached kind:10002 read relays
//! and, when that inbox is not yet cached, fails closed — UNLESS the interest
//! is `Tailing + Global`, in which case a cold-start gate
//! (`is_bootstrap_inbox_eligible`) reroutes it onto the app's configured
//! `bootstrap_content_relays` until the real inbox lands. That gate does not
//! fire for `ActiveAccount` scope. A fresh account that has never published
//! (or cached) its own kind:10002 — e.g. two brand-new identities on a single
//! ad-hoc relay wired via `add_relay`, never a NIP-65 relay list — therefore
//! got ZERO relay entries for this interest: the kind:9321 REQ was never sent
//! to any relay, so an inbound nutzap sitting on the relay never reached
//! `on_wallet_event`, not even as a rejected/unverifiable row (#2942). Opening
//! `Global` instead mirrors `nmp_nip57::self_zap_receipts_interest` exactly —
//! the same `#p`=self / `Nip65ReadRelays` / public-content shape, already
//! proven to need (and get) the cold-start bootstrap lane. `wallet_self_authored_shape`
//! is an `authors`=self filter (no `#p` tag), which Case A routes via the
//! account's own write relays / `app_relays` unconditionally (no `Global`-only
//! gate applies), so it keeps `ActiveAccount` scope — unaffected by this bug.
//!
//! # `MintResult` — no live producer yet
//!
//! [`Self::deliver_mint_result`] exists and is tested, but nothing calls it
//! today: `CashuWalletBackend`'s own `ProtocolCommand` workers
//! (`CashuDepositQuoteCommand`/`CashuCompleteDepositCommand`) map mint
//! responses directly against the richer quote/bolt11/proof data they hold,
//! never through the coarser `MintResult` seam (see
//! `backend::cashu::CashuWalletBackend::on_mint_result`'s doc comment). This
//! is an explicit, documented deferral, not a silent gap: when a future
//! backend or worker does need the generic seam, the wiring to reach
//! `WalletBackend::on_mint_result` already exists and is exercised by tests.

use std::sync::Arc;

use nmp_core::slots::ActiveAccountSlot;
use nmp_core::substrate::{
    IdentityChangeRegistrar, KernelEvent, ObservedProjectionReconciler, ObservedProjectionRegistrar,
};
use nmp_core::{CommandSender, ObservedProjectionSink};

use crate::backend::{MintResult, WalletBackendContext};
use crate::interests::{nutzap_receipts_shape, wallet_self_authored_shape};
use crate::projection::WalletProjection;
use crate::selector::WalletBackendSelector;
use crate::{WalletBackendId, WalletProjectionScope};

/// `ObservedProjection::scope` value for `InterestScope::ActiveAccount`
/// (`nmp-core/src/subs/interest_builder.rs`: `0 = ActiveAccount`). Used for
/// `wallet_self_authored_shape`, an `authors`=self interest with no
/// cold-start bootstrap dependency (see module docs).
const SCOPE_ACTIVE_ACCOUNT: u32 = 0;

/// `ObservedProjection::scope` value for `InterestScope::Global` (any nonzero
/// value maps to `Global` — `1` is the conventional choice, mirroring
/// `nmp_nip57`'s own zap-receipts runtime). Used for `nutzap_receipts_shape`,
/// a `#p`=self / `Nip65ReadRelays` interest that needs the planner's Case C
/// cold-start bootstrap-inbox gate — see module docs (#2942).
const SCOPE_GLOBAL: u32 = 1;

/// Bounded cache replay on (re)open — generous enough to hydrate a returning
/// account's wallet state and recent nutzap history without an unbounded
/// read. Mirrors the order of magnitude of `nmp-wot`'s own bootstrap replay
/// limit (512) and `nmp-wallet::projection::MAX_WALLET_PROJECTION_ROWS`'s
/// row cap.
const REPLAY_LIMIT: usize = 512;

/// Routes observed `KernelEvent`s to every registered backend's
/// `on_wallet_event`. A durable wallet event is not addressed to a single
/// backend the way a dispatched intent is (e.g. a nutzap receipt is a Cashu
/// concern regardless of whether NWC is the preferred `pay_bolt11` backend),
/// so every registered backend sees every observed event; each backend's own
/// `on_wallet_event` already no-ops events outside its concern (see their doc
/// comments).
struct WalletEventSink {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
    tx: CommandSender,
}

impl ObservedProjectionSink for WalletEventSink {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let account_pubkey = self.active_pubkey.lock().ok().and_then(|slot| slot.clone());
        let preferred = self.selector.preferred();
        // D9-style choice: no wall-clock read is reachable from an observer
        // callback (only `ProtocolCommandContext::now_secs()` inside a
        // dispatched `ProtocolCommand::run` has one). `WalletBackendContext`'s
        // `now_secs` is consumed today only by
        // `CashuWalletBackend::start_intent`'s correlation-id-absent
        // fallback label (never reached from this event-driven path, which
        // never carries a `correlation_id` at all) — so the accepted event's
        // own `created_at` is a sound, deterministic stand-in rather than a
        // fresh `SystemTime::now()` read.
        let ctx = WalletBackendContext {
            now_secs: event.created_at,
            selected_backend: preferred.as_ref(),
            account_pubkey: account_pubkey.as_deref(),
        };
        for backend in self.selector.backends() {
            for cmd in backend.on_wallet_event(ctx, event) {
                let _ = self.tx.send(cmd);
            }
        }
    }
}

/// The wallet's actor-side runtime controller. See module docs.
pub struct WalletRuntime {
    selector: Arc<WalletBackendSelector>,
    active_pubkey: ActiveAccountSlot,
    tx: CommandSender,
}

impl WalletRuntime {
    /// Construct the runtime and wire its two identity-reactive observed
    /// projections onto `app` — mirrors
    /// `nmp-nip51::register_mute_runtime`'s register + identity-hook +
    /// eager-sync recipe (see module docs).
    pub fn new(
        selector: Arc<WalletBackendSelector>,
        active_pubkey: ActiveAccountSlot,
        tx: CommandSender,
        app: &(impl ObservedProjectionRegistrar + IdentityChangeRegistrar),
    ) -> Self {
        let sink: Arc<dyn ObservedProjectionSink> = Arc::new(WalletEventSink {
            selector: Arc::clone(&selector),
            active_pubkey: Arc::clone(&active_pubkey),
            tx: tx.clone(),
        });

        let nutzap_pubkey = Arc::clone(&active_pubkey);
        let nutzap_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            Arc::clone(&sink),
            "nmp.wallet.nutzap_receipts",
            SCOPE_GLOBAL,
            REPLAY_LIMIT,
            Arc::new(move || {
                let pubkey = nutzap_pubkey.lock().ok()?.clone()?;
                Some(nutzap_receipts_shape(&pubkey))
            }),
        );
        let self_authored_pubkey = Arc::clone(&active_pubkey);
        let self_authored_reconciler = ObservedProjectionReconciler::new(
            app.observed_projection_registrar_handle(),
            sink,
            "nmp.wallet.self_authored",
            SCOPE_ACTIVE_ACCOUNT,
            REPLAY_LIMIT,
            Arc::new(move || {
                let pubkey = self_authored_pubkey.lock().ok()?.clone()?;
                Some(wallet_self_authored_shape(&pubkey))
            }),
        );

        let nutzap_for_identity = nutzap_reconciler.clone();
        let self_authored_for_identity = self_authored_reconciler.clone();
        app.register_identity_change_observer(move |_| {
            nutzap_for_identity.sync();
            self_authored_for_identity.sync();
        });
        // Eager sync for cold start: the account may already be active
        // before this registration runs.
        nutzap_reconciler.sync();
        self_authored_reconciler.sync();

        Self {
            selector,
            active_pubkey,
            tx,
        }
    }

    #[must_use]
    pub fn selector(&self) -> &Arc<WalletBackendSelector> {
        &self.selector
    }

    /// Build the merged wallet projection (backend selection + capability
    /// union + concatenated bounded rows — see
    /// `WalletBackendSelector::snapshot`).
    #[must_use]
    pub fn snapshot(&self) -> WalletProjection {
        self.selector.snapshot(WalletProjectionScope {
            include_history: true,
            include_receive_rows: true,
        })
    }

    /// Deliver a `MintResult` to a specific backend's `on_mint_result`,
    /// forwarding whatever `ActorCommand`s it returns. See module docs — no
    /// caller exists yet.
    pub fn deliver_mint_result(&self, backend_id: &WalletBackendId, result: MintResult) {
        let Some(backend) = self.selector.backend_by_id(backend_id) else {
            return;
        };
        let account_pubkey = self.active_pubkey.lock().ok().and_then(|slot| slot.clone());
        let preferred = self.selector.preferred();
        let ctx = WalletBackendContext {
            now_secs: 0,
            selected_backend: preferred.as_ref(),
            account_pubkey: account_pubkey.as_deref(),
        };
        for cmd in backend.on_mint_result(ctx, result) {
            let _ = self.tx.send(cmd);
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
