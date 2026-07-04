//! W4–W7 (#2908, epic #2864) — the single composition-root entry point for
//! `nmp-wallet`: backend selection + registration, the canonical
//! `nmp.wallet.*` action modules this crate owns, and the actor/runtime
//! wiring (read interests + observer, see `runtime.rs`).
//!
//! # `nmp.wallet.{connect,disconnect,pay_invoice}` stay in `nmp-nip47`
//!
//! [`register`] calls [`nmp_nip47::register`] once — exactly as
//! `nmp-native-runtime`'s composition-root builder did directly before this
//! function existed — to obtain the [`nmp_nip47::Handles`] needed to
//! construct a live [`NwcWalletBackend`], but it does **not** re-register
//! `nmp.wallet.connect`/`nmp.wallet.disconnect`/`nmp.wallet.pay_invoice`:
//! `nmp_nip47::register` already registers those three `ActionModule`s
//! itself, unconditionally, under the exact same namespace strings this
//! crate's `ownership.rs` claims. A second registration under those names
//! would either collide (`ActionRegistrar::register_action` errors on an
//! app-vs-app namespace collision) or require `nmp-nip47` to stop
//! self-registering them — a "move the ActionModule out of nmp-nip47"
//! change `nmp-nip47/src/ownership.rs`'s own note explicitly defers to epic
//! #2864 Phase 2 (NWC consolidation). This wave's new backend-selecting
//! dispatch therefore covers exactly `select_backend` (new) plus the
//! wholly-new `cashu.*`/`nutzap.*` families — none of which any crate
//! registers today, so there is no collision risk. A direct
//! `nmp.wallet.pay_invoice` tap bypasses backend selection and reaches
//! `nmp-nip47`'s runtime directly until Phase 2 moves that `ActionModule`
//! here; the NWC backend the selector holds is still live and reachable via
//! `WalletIntent::PayBolt11` for anything ELSE that dispatches through the
//! selector (`nmp-wallet::payment_port`'s zap wiring is a separate,
//! not-yet-connected wave — see that module's doc comment).
//!
//! # No typed `"wallet"` snapshot projection registered here yet
//!
//! `SnapshotProjectionRegistrar` (required transitively, to satisfy
//! `nmp_nip47::register`'s own bound) is part of this function's bound, but
//! this wave does not call `register_typed_snapshot_projection` for a merged
//! multi-backend wallet projection: every existing typed projection in this
//! workspace is a hand-authored FlatBuffers schema
//! (`crates/<crate>/schema/*.fbs`) processed by the pinned-`flatc` codegen
//! script (`ci/regenerate-flatbuffers.sh`) — schema design, not composition
//! wiring, and a new `wallet_projection.fbs` covering `WalletProjection`'s
//! nested balance/history/receive-row vectors is a properly-sized follow-up
//! on its own. [`WalletRuntime::snapshot`] already builds the correct merged
//! [`crate::projection::WalletProjection`] and is unit-tested; wiring it into
//! a typed sidecar is the deferred piece (tracked as a fast-follow issue —
//! see the PR this landed in). `nmp-nip47`'s own existing `"wallet"` typed
//! projection (its `WalletStatus` shape) keeps being registered by
//! `nmp_nip47::register` above, unaffected either way.

use std::sync::Arc;

use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, ObservedProjectionRegistrar,
    RegistrationError, RelayTextInterceptorRegistrar, SnapshotProjectionRegistrar,
};

use crate::action::{
    CashuCompleteDepositModule, CashuCreateModule, CashuDepositQuoteModule, CashuRecoverModule,
    NutzapPublishInfoModule, NutzapRedeemModule, NutzapSendModule, SelectBackendModule,
};
use crate::backend::cashu::CashuWalletBackend;
use crate::backend::nwc::NwcWalletBackend;
use crate::backend::WalletBackend;
use crate::runtime::WalletRuntime;
use crate::selector::WalletBackendSelector;

#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Forwarded to `nmp_nip47::Config` for the durable NWC payment store
    /// (see that crate's `register` docs). `None` keeps NWC in-memory-only.
    pub storage_path: Option<String>,
}

impl Config {
    #[must_use]
    pub fn new(storage_path: Option<String>) -> Self {
        Self { storage_path }
    }
}

pub struct Handles {
    /// The per-app NWC runtime handle `nmp_nip47::register` constructed —
    /// kept so the composition-root caller can wire
    /// `nmp_nip47::wallet_payment_port(handles.nwc_wallet)` into the NIP-57
    /// zap auto-chain exactly as it did before this crate's `register`
    /// existed (see `crates/nmp-native-runtime/src/builder/wallet.rs`).
    pub nwc_wallet: nmp_nip47::WalletRuntimeHandle,
    /// The installed wallet runtime — owns backend selection and drives
    /// `on_wallet_event`/`on_mint_result` (see `runtime.rs`). The identity-
    /// reactive read-interest wiring keeps working regardless of what the
    /// caller does with this handle (each reconciler's closure holds its own
    /// `Arc` clones, captured by `app.register_identity_change_observer`
    /// — see `WalletRuntime::new`). This handle is offered ADDITIONALLY so a
    /// caller that retains it can reach `WalletRuntime::snapshot`/
    /// `deliver_mint_result` later (e.g. to wire a typed "wallet" snapshot
    /// projection once one exists). `nmp-native-runtime`'s
    /// `NmpAppBuilder::with_wallet` (#2919) stashes it in a builder-level
    /// slot and re-exposes it via `NmpAppBuilder::wallet_runtime()`, so a
    /// Rust composition root going through the builder can retrieve a clone
    /// and call `.snapshot()` — the same access a caller that invokes this
    /// `register` fn directly (e.g. as its own composition root) already had.
    pub runtime: Arc<WalletRuntime>,
}

/// Register the wallet composition stack on `app`. See module docs for what
/// is and is not registered this wave.
pub fn register(
    app: &mut (impl ActionRegistrar
              + RelayTextInterceptorRegistrar
              + SnapshotProjectionRegistrar
              + ObservedProjectionRegistrar
              + IdentityChangeRegistrar
              + HostCapabilities),
    config: Config,
) -> Result<Handles, RegistrationError> {
    // 1. NWC runtime — unchanged registration (see module docs): its own
    //    connect/disconnect/pay_invoice actions, interceptor, and "wallet"
    //    projection all still register exactly as before. We only need the
    //    returned `Handles` to build a live `NwcWalletBackend`.
    let nip47_handles = nmp_nip47::register(app, nmp_nip47::Config::new(config.storage_path))?;

    // 2. Backend registry (W4).
    let nwc_backend: Arc<dyn WalletBackend> = Arc::new(NwcWalletBackend::new(
        nip47_handles.wallet.clone(),
        nip47_handles.status.clone(),
    ));
    // Kept as a concrete `Arc<CashuWalletBackend>` (not yet erased to
    // `Arc<dyn WalletBackend>`) so step 3a below can call its
    // Cashu-specific `reset()` — the `WalletBackend` trait itself has no
    // (and should have no) generic "forget everything" method; NWC's
    // connection state is not identity-scoped the same way, so nothing
    // analogous is needed for `nwc_backend`.
    let cashu_backend = Arc::new(CashuWalletBackend::new());
    let selector = Arc::new(WalletBackendSelector::new(vec![
        nwc_backend,
        Arc::clone(&cashu_backend) as Arc<dyn WalletBackend>,
    ]));

    // 3a. Cross-account data-leak fix: `CashuWalletBackend` is constructed
    //    once per app instance (above), not once per signed-in account, but
    //    its state (mints, Cashu P2PK pubkey, balances, pending deposits) is
    //    NIP-44-encrypted-to-a-specific-identity material. Without this, a
    //    Nostr account switch within one running app would leave the
    //    previous account's wallet state visible to — and, via
    //    `complete_deposit`, completable as — the newly active account.
    //    Reset on every active-account change, mirroring how
    //    `nmp-nip51::register_mute_runtime` resets `MuteListProjection` on
    //    the same signal.
    app.register_identity_change_observer(move |_| cashu_backend.reset());

    // 3. Canonical `nmp.wallet.*` action modules this crate owns this wave
    //    (W5) — `select_backend` plus the Cashu/nutzap families. Each holds
    //    its own `Arc` clone of the selector (and, where an intent needs
    //    identity, of the active-pubkey slot), never a process-global.
    let active_pubkey = app.active_pubkey();
    app.register_action(SelectBackendModule::new(Arc::clone(&selector)))?;
    app.register_action(CashuCreateModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuRecoverModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuDepositQuoteModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuCompleteDepositModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(NutzapPublishInfoModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(NutzapSendModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(NutzapRedeemModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;

    // 4/5/6. Actor/runtime wiring (W6/W7): identity-reactive read interests
    //    (kind:9321 `#p`=self, kind:10019/17375/7375/7376/7374 `authors`=self)
    //    plus the observer that routes matching `KernelEvent`s into each
    //    registered backend's `on_wallet_event`.
    let runtime = Arc::new(WalletRuntime::new(
        Arc::clone(&selector),
        active_pubkey,
        app.actor_sender(),
        app,
    ));

    Ok(Handles {
        nwc_wallet: nip47_handles.wallet,
        runtime,
    })
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
