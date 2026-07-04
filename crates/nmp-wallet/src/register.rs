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
//! # Typed `"wallet.merged"` snapshot projection (#2915)
//!
//! This function registers a typed FlatBuffers snapshot projection for the
//! MERGED multi-backend [`crate::projection::WalletProjection`] that
//! [`WalletRuntime::snapshot`] builds (backend selection + capability union +
//! concatenated bounded rows). It is registered under the DISTINCT key
//! `"wallet.merged"` — deliberately NOT `"wallet"`, which `nmp-nip47` still owns
//! for its single-backend NWC `WalletStatus` (`NWST`) shape. Choosing a fresh
//! key means nothing has to be moved out of / broken in `nmp-nip47`: the two
//! typed sidecars coexist, each host decoding whichever it understands (an
//! NWC-only host keeps reading `NWST` `"wallet"`; a merged-wallet host reads
//! `NWMP` `"wallet.merged"`). The `nmp.wallet.*` action namespaces and the
//! `wallet` projection family are owned by this crate's `ownership.rs`; the new
//! `"wallet.merged"` key adds its own exclusive `projection.wallet.merged`
//! claim there, cited by the `DeclaredProjectionKey` this function passes and by
//! the `PROJECTION_CONTRACT` row (`crates/nmp-codegen`). The schema
//! (`crates/nmp-wallet/schema/wallet_projection.fbs`) and its encode/decode
//! codec (`crate::projection_wire`) follow the `NotificationsSnapshot` /
//! `ModularTimelineSnapshot` vector-of-tables precedent, generated ONLY through
//! `ci/regenerate-flatbuffers.sh`. `nmp-nip47`'s own existing `"wallet"` typed
//! projection keeps being registered by `nmp_nip47::register` above, unaffected.

use std::sync::Arc;

use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, ObservedProjectionRegistrar,
    RegistrationError, RelayTextInterceptorRegistrar, SnapshotProjectionRegistrar,
};

use crate::journal::{FsWalletWalStore, WalletWalStore};

use crate::action::{
    CashuCompleteDepositModule, CashuCreateModule, CashuCrossMintTransferModule,
    CashuDepositQuoteModule, CashuRecoverModule, CashuSetMintsModule, NutzapPublishInfoModule,
    NutzapRedeemModule, NutzapSendModule, SelectBackendModule,
};
use crate::backend::cashu::CashuWalletBackend;
use crate::backend::nwc::NwcWalletBackend;
use crate::backend::WalletBackend;
use crate::discovery_runtime::MintDiscoveryRuntime;
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
    /// The installed NIP-87 mint discovery runtime (#2880). Owns the
    /// identity-reactive read interests for kind:38172 announcements +
    /// kind:38000 recommendations and the viewer's follow/mute graph, and
    /// produces the web-of-trust-scoped, capability-fail-closed
    /// discovered-mints projection via [`MintDiscoveryRuntime::snapshot`]. Held
    /// so a composition root can query discovered mints (the same
    /// runtime-holds-projection access as `runtime` above).
    pub mint_discovery: Arc<MintDiscoveryRuntime>,
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
    // The durable pre-publish WAL store (PR-1 of #2910/#2960/#2931) is
    // fs-backed exactly when a persistent `storage_path` is configured —
    // mirroring `nmp-nip47`'s own fs-vs-memory decision for `FsPaymentStore`
    // (`register.rs`: `config.storage_path.filter(|p| !p.trim().is_empty())`).
    // `None` keeps the wallet journal in-memory-only, the same accepted
    // tradeoff NWC's payment store already has today.
    let wal_store: Option<Arc<dyn WalletWalStore>> = config
        .storage_path
        .clone()
        .filter(|p| !p.trim().is_empty())
        .map(|p| Arc::new(FsWalletWalStore::new(p)) as Arc<dyn WalletWalStore>);

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
    let cashu_backend = Arc::new(CashuWalletBackend::with_wal_store(wal_store));
    let selector = Arc::new(WalletBackendSelector::new(vec![
        nwc_backend,
        Arc::clone(&cashu_backend) as Arc<dyn WalletBackend>,
    ]));

    let active_pubkey = app.active_pubkey();

    // 3a. Cross-account data-leak fix + durable-WAL restore (PR-1 of
    //    #2910/#2960/#2931). `CashuWalletBackend` is constructed once per app
    //    instance (above), not once per signed-in account, but its state
    //    (mints, Cashu P2PK pubkey, balances, pending deposits) is
    //    NIP-44-encrypted-to-a-specific-identity material. Without the reset, a
    //    Nostr account switch within one running app would leave the previous
    //    account's wallet state visible to — and, via `complete_deposit`,
    //    completable as — the newly active account. Reset on every
    //    active-account change, mirroring how `nmp-nip51::register_mute_runtime`
    //    resets `MuteListProjection` on the same signal.
    //
    //    Immediately after the reset, `restore_from_wal` rehydrates the fresh
    //    journal from the durable WAL for the newly active account (keying
    //    write-through under it and self-healing terminal rows — see
    //    `restore_into_journal`'s #2931 rule). This is the identity-becomes-
    //    active hook the WAL restore belongs on: at bare backend construction
    //    there is no account yet.
    let identity_backend = Arc::clone(&cashu_backend);
    // The actor mail sender the restore path forwards its `ResumeDepositCommand`s
    // onto (PR-2 of #2910): `restore_from_wal` rebuilds `pending_deposits` and
    // returns one re-drive command per deposit past the mint, which must run
    // through the actor as a `ProtocolCommand` (relays + off-actor-thread D8).
    let restore_tx = app.actor_sender();
    let observer_tx = restore_tx.clone();
    app.register_identity_change_observer(move |new_pubkey| {
        identity_backend.reset();
        if let Some(pubkey) = new_pubkey {
            for cmd in identity_backend.restore_from_wal(&pubkey) {
                let _ = observer_tx.send(cmd);
            }
        }
    });
    // Eager cold-start restore: the account may already be active before this
    // registration runs (the identity observer fires only on a *change*), so a
    // process restart with a signed-in account still rehydrates its WAL — and
    // re-drives any deposit caught mid-chain by the crash.
    if let Some(pubkey) = active_pubkey.lock().ok().and_then(|slot| slot.clone()) {
        for cmd in cashu_backend.restore_from_wal(&pubkey) {
            let _ = restore_tx.send(cmd);
        }
    }

    // 3. Canonical `nmp.wallet.*` action modules this crate owns this wave
    //    (W5) — `select_backend` plus the Cashu/nutzap families. Each holds
    //    its own `Arc` clone of the selector (and, where an intent needs
    //    identity, of the active-pubkey slot), never a process-global.
    app.register_action(SelectBackendModule::new(Arc::clone(&selector)))?;
    app.register_action(CashuCreateModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuRecoverModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuSetMintsModule::new(
        Arc::clone(&selector),
        Arc::clone(&active_pubkey),
    ))?;
    app.register_action(CashuCrossMintTransferModule::new(
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
        Arc::clone(&active_pubkey),
        app.actor_sender(),
        app,
    ));

    // 7. Typed `"wallet.merged"` snapshot projection (#2915) — the merged
    //    multi-backend projection `WalletRuntime::snapshot` builds, emitted as a
    //    typed FlatBuffers sidecar under a DISTINCT key from `nmp-nip47`'s
    //    single-backend `"wallet"` (`NWST`) sidecar (see module docs). The
    //    closure holds its own `Arc<WalletRuntime>` clone (no process-global) and
    //    is a read-only, non-blocking snapshot producer (D8).
    let projection_runtime = Arc::clone(&runtime);
    app.register_typed_snapshot_projection(
        nmp_ownership::DeclaredProjectionKey::framework(
            crate::projection_wire::PROJECTION_KEY,
            "projection.wallet.merged",
        ),
        move || Some(wallet_merged_typed_projection(&projection_runtime)),
    );

    // 8. NIP-87 mint discovery (#2880): identity-reactive read interests for
    //    kind:38172 announcements + kind:38000 recommendations plus the
    //    account's follow/mute graph, aggregated (WoT-scoped, fail-closed on
    //    mints missing the nutzap NUTs) into the discovered-mints projection.
    let mint_discovery = Arc::new(MintDiscoveryRuntime::new(active_pubkey, app));

    Ok(Handles {
        nwc_wallet: nip47_handles.wallet,
        runtime,
        mint_discovery,
    })
}

/// Build the typed `"wallet.merged"` sidecar entry from the live runtime's
/// merged snapshot (#2915). Extracted from the
/// `register_typed_snapshot_projection` closure so the registration's schema
/// identity (`key` / `schema_id` / `file_identifier` / version) and the encode
/// are unit-testable without spinning the actor.
///
/// Unlike `nmp-nip47`'s `wallet_typed_projection` (which returns `None` when no
/// wallet is connected), this always emits a row: `WalletRuntime::snapshot`
/// yields a well-formed empty/`NotConfigured` projection when nothing is
/// configured, and emitting it keeps the host cache authoritative (an omitted
/// key retains the last decoded value under incremental apply — see ADR-0070).
#[must_use]
pub fn wallet_merged_typed_projection(runtime: &WalletRuntime) -> nmp_core::TypedProjectionData {
    let projection = runtime.snapshot();
    nmp_core::TypedProjectionData {
        key: crate::projection_wire::PROJECTION_KEY.to_string(),
        schema_id: crate::projection_wire::SCHEMA_ID.to_string(),
        schema_version: crate::projection_wire::SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(crate::projection_wire::FILE_IDENTIFIER)
            .into_owned(),
        payload: crate::projection_wire::encode_wallet_projection(&projection),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
