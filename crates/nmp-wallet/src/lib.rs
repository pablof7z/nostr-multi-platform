//! `nmp-wallet` — wallet composition owner for NMP apps.
//!
//! This crate owns the wallet product surface described in
//! `docs/architecture/nip60-nip61-wallet-design.md`: action namespaces, backend
//! capability flags, the bounded `"wallet"` projection shape, operation-journal
//! state, the unified backend seam, and the payment-port adapter NIP-57 uses.
//!
//! It deliberately does not open relay sockets, perform mint HTTP, hold native
//! UI state, or own NIP-specific event codecs. Protocol mechanics remain in
//! `nmp-nip47`, `nmp-nip57`, and `nmp-nip60`; app shells render this crate's
//! projection and dispatch typed actions only.

#![forbid(unsafe_code)]

pub mod backend;
pub mod capability;
pub mod journal;
pub mod ownership;
pub mod payment_port;
pub mod projection;

pub use backend::{
    MintResult, WalletBackend, WalletBackendContext, WalletBackendId, WalletBackendSnapshot,
    WalletIntent, WalletProjectionScope,
};
pub use capability::{WalletCapabilities, WalletCapability};
pub use journal::{
    WalletConsumedInput, WalletJournalError, WalletOperation, WalletOperationId,
    WalletOperationJournal, WalletOperationKind, WalletOperationState,
};
pub use payment_port::{WalletBolt11Payment, WalletPaymentCommandFactory, WalletPaymentPort};
pub use projection::{
    WalletBalanceRow, WalletHistoryKind, WalletHistoryRow, WalletProjection, WalletReadiness,
    WalletReceiveRow, MAX_WALLET_PROJECTION_ROWS, WALLET_PROJECTION_KEY,
};

pub const ACTION_SELECT_BACKEND: &str = "nmp.wallet.select_backend";
pub const ACTION_NWC_CONNECT: &str = "nmp.wallet.nwc.connect";
pub const ACTION_NWC_DISCONNECT: &str = "nmp.wallet.nwc.disconnect";
pub const ACTION_LEGACY_NWC_CONNECT: &str = "nmp.wallet.connect";
pub const ACTION_LEGACY_NWC_DISCONNECT: &str = "nmp.wallet.disconnect";
pub const ACTION_PAY_INVOICE: &str = "nmp.wallet.pay_invoice";
pub const ACTION_CASHU_CREATE: &str = "nmp.wallet.cashu.create";
pub const ACTION_CASHU_RECOVER: &str = "nmp.wallet.cashu.recover";
pub const ACTION_CASHU_DEPOSIT_QUOTE: &str = "nmp.wallet.cashu.deposit_quote";
pub const ACTION_CASHU_COMPLETE_DEPOSIT: &str = "nmp.wallet.cashu.complete_deposit";
pub const ACTION_NUTZAP_PUBLISH_INFO: &str = "nmp.wallet.nutzap.publish_info";
pub const ACTION_NUTZAP_SEND: &str = "nmp.wallet.nutzap.send";
pub const ACTION_NUTZAP_REDEEM: &str = "nmp.wallet.nutzap.redeem";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_action_names_are_backend_explicit_except_unified_pay_invoice() {
        let names = [
            ACTION_SELECT_BACKEND,
            ACTION_NWC_CONNECT,
            ACTION_NWC_DISCONNECT,
            ACTION_LEGACY_NWC_CONNECT,
            ACTION_LEGACY_NWC_DISCONNECT,
            ACTION_PAY_INVOICE,
            ACTION_CASHU_CREATE,
            ACTION_CASHU_RECOVER,
            ACTION_CASHU_DEPOSIT_QUOTE,
            ACTION_CASHU_COMPLETE_DEPOSIT,
            ACTION_NUTZAP_PUBLISH_INFO,
            ACTION_NUTZAP_SEND,
            ACTION_NUTZAP_REDEEM,
        ];
        assert!(names.iter().all(|name| name.starts_with("nmp.wallet.")));
        assert!(names.iter().any(|name| name.contains(".cashu.")));
        assert!(names.iter().any(|name| name.contains(".nutzap.")));
        assert!(names.iter().any(|name| name.contains(".nwc.")));
    }
}
