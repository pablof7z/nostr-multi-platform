//! `nmp-wallet` — wallet composition owner for NMP apps.
//!
//! This crate owns the wallet product surface described in
//! `docs/architecture/nip60-nip61-wallet-design.md`: action namespaces, backend
//! capability flags, the bounded `"wallet"` projection shape, operation-journal
//! state, and the unified backend seam. It selects which backend's
//! `PaymentPort` adapter NIP-57 pays through; the adapter itself is owned by
//! the crate implementing that backend (`nmp-nip47` for NWC today).
//!
//! It deliberately does not open relay sockets, perform mint HTTP, hold native
//! UI state, or own NIP-specific event codecs. Protocol mechanics remain in
//! `nmp-nip47`, `nmp-nip57`, and `nmp-nip60`; app shells render this crate's
//! projection and dispatch typed actions only.
//!
// `deny` (not `forbid`) so the eight generated FlatBuffers bindings modules
// `wire.rs` carries (#2920, epic #2864) may opt back in via
// `#[allow(unsafe_code)]` — FlatBuffers accessors are intrinsically `unsafe`;
// `forbid` cannot be locally overridden. All hand-written code in this crate
// remains unsafe-free — the allow is scoped to the `#[path]`-included
// generated files only, mirroring `nmp-content`'s own `wire::typed_fb` posture.
#![deny(unsafe_code)]

pub mod action;
pub mod backend;
pub mod capability;
pub mod discovery_runtime;
mod fail_closed;
pub mod interests;
pub mod journal;
pub mod mint_discovery;
pub mod ownership;
pub mod payment_port;
pub mod projection;
pub mod projection_wire;
pub mod register;
pub mod runtime;
pub mod selector;
pub mod ui_codes;
mod wire;

pub use action::{
    CashuCompleteDepositAction, CashuCreateAction, CashuDepositQuoteAction, CashuRecoverAction,
    CashuSetMintsAction, NutzapPublishInfoAction, NutzapRedeemAction, NutzapSendAction,
    SelectBackendAction,
};
pub use backend::cashu::{CashuWalletBackend, CASHU_BACKEND_ID};
pub use backend::nwc::{NwcWalletBackend, NWC_BACKEND_ID};
pub use backend::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
pub use capability::{WalletCapabilities, WalletCapability};
pub use discovery_runtime::MintDiscoveryRuntime;
pub use journal::{
    CorrelationId, DeleteCause, HistoryFactSeed, MintUrl, ProofAtom, ProofRef, ProofVerdict,
    Provenance, PubkeyRef, RelayRef, WalletApplySummary, WalletBalanceKey, WalletCauseIndex,
    WalletConsumedInput, WalletDeltaRing, WalletDerivedState, WalletEventId, WalletFact,
    WalletJournalError, WalletLedger, WalletOperation, WalletOperationId, WalletOperationJournal,
    WalletOperationKind, WalletOperationState, WalletSagaEvent, WalletTrailEntry, WalletUnit,
};
pub use mint_discovery::{
    aggregate_discovered_mints, DiscoveredMint, MintDiscoveryPolicy, MintDiscoveryProjection,
    MintDiscoveryStore, MAX_DISCOVERED_MINTS,
};
pub use payment_port::{
    WalletBackendPaymentCommandFactory, WalletBackendPaymentRouter, WalletBolt11Payment,
};
pub use projection::{
    WalletBalanceRow, WalletHistoryKind, WalletHistoryRow, WalletProjection, WalletReadiness,
    WalletReceiveRow, MAX_WALLET_PROJECTION_ROWS, WALLET_PROJECTION_KEY,
};
pub use projection_wire::{
    decode_wallet_projection, encode_wallet_projection,
    PROJECTION_KEY as WALLET_MERGED_PROJECTION_KEY, SCHEMA_ID as WALLET_MERGED_SCHEMA_ID,
    SCHEMA_VERSION as WALLET_MERGED_SCHEMA_VERSION,
};
pub use register::{register, Config, Handles};
pub use runtime::WalletRuntime;
pub use selector::{SelectorError, WalletBackendSelector};

pub const ACTION_SELECT_BACKEND: &str = "nmp.wallet.select_backend";
// `nmp-nip47` is today's only implementation of NWC connect/disconnect, under
// these exact names — there is no second, already-real "nmp.wallet.nwc.*"
// implementation to be canonical relative to. Renaming to a backend-qualified
// `nmp.wallet.nwc.connect`/`nmp.wallet.nwc.disconnect` is epic #2864 Phase 2
// (NWC consolidation) work: it requires moving the `ActionModule` + wire
// schema registration out of `nmp-nip47`, which is that crate's lane, not
// this one's. Declaring both an aspirational new name and this real one as a
// "canonical vs. legacy alias" pair before that move lands would just be a
// compat alias with extra steps — so there is exactly one name per action.
pub const ACTION_NWC_CONNECT: &str = "nmp.wallet.connect";
pub const ACTION_NWC_DISCONNECT: &str = "nmp.wallet.disconnect";
pub const ACTION_PAY_INVOICE: &str = "nmp.wallet.pay_invoice";
pub const ACTION_CASHU_CREATE: &str = "nmp.wallet.cashu.create";
pub const ACTION_CASHU_RECOVER: &str = "nmp.wallet.cashu.recover";
pub const ACTION_CASHU_DEPOSIT_QUOTE: &str = "nmp.wallet.cashu.deposit_quote";
pub const ACTION_CASHU_COMPLETE_DEPOSIT: &str = "nmp.wallet.cashu.complete_deposit";
/// #2997 — key-preserving wallet config edit: replaces the kind:17375
/// accepted-mint list, carrying the existing Cashu P2PK privkey forward
/// unchanged (never rotates it, unlike `cashu.create`'s `WalletConfig::generate`).
pub const ACTION_CASHU_SET_MINTS: &str = "nmp.wallet.cashu.set_mints";
pub const ACTION_NUTZAP_PUBLISH_INFO: &str = "nmp.wallet.nutzap.publish_info";
pub const ACTION_NUTZAP_SEND: &str = "nmp.wallet.nutzap.send";
pub const ACTION_NUTZAP_REDEEM: &str = "nmp.wallet.nutzap.redeem";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_action_names_cover_the_declared_namespaces() {
        let names = [
            ACTION_SELECT_BACKEND,
            ACTION_NWC_CONNECT,
            ACTION_NWC_DISCONNECT,
            ACTION_PAY_INVOICE,
            ACTION_CASHU_CREATE,
            ACTION_CASHU_RECOVER,
            ACTION_CASHU_DEPOSIT_QUOTE,
            ACTION_CASHU_COMPLETE_DEPOSIT,
            ACTION_CASHU_SET_MINTS,
            ACTION_NUTZAP_PUBLISH_INFO,
            ACTION_NUTZAP_SEND,
            ACTION_NUTZAP_REDEEM,
        ];
        assert!(names.iter().all(|name| name.starts_with("nmp.wallet.")));
        assert!(names.iter().any(|name| name.contains(".cashu.")));
        assert!(names.iter().any(|name| name.contains(".nutzap.")));
    }

    /// No compat aliases: every wallet action constant must name a distinct
    /// namespace. A repeated string value would mean a "canonical" name and
    /// a "legacy" name for the same concept coexist again.
    #[test]
    fn no_action_namespace_is_duplicated_as_a_compatibility_alias() {
        let names = [
            ACTION_SELECT_BACKEND,
            ACTION_NWC_CONNECT,
            ACTION_NWC_DISCONNECT,
            ACTION_PAY_INVOICE,
            ACTION_CASHU_CREATE,
            ACTION_CASHU_RECOVER,
            ACTION_CASHU_DEPOSIT_QUOTE,
            ACTION_CASHU_COMPLETE_DEPOSIT,
            ACTION_CASHU_SET_MINTS,
            ACTION_NUTZAP_PUBLISH_INFO,
            ACTION_NUTZAP_SEND,
            ACTION_NUTZAP_REDEEM,
        ];
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "every wallet action constant must name a distinct namespace"
        );
    }
}
