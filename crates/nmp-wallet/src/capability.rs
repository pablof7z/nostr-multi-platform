use serde::{Deserialize, Serialize};

use crate::{
    ACTION_CASHU_COMPLETE_DEPOSIT, ACTION_CASHU_CREATE, ACTION_CASHU_DEPOSIT_QUOTE,
    ACTION_CASHU_RECOVER, ACTION_NUTZAP_PUBLISH_INFO, ACTION_NUTZAP_REDEEM, ACTION_NUTZAP_SEND,
    ACTION_NWC_CONNECT, ACTION_NWC_DISCONNECT, ACTION_PAY_INVOICE,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalletCapability {
    PayBolt11,
    CreateCashuWallet,
    PublishNutzapInfo,
    SendNutzap,
    RedeemNutzap,
    DepositCashu,
    MeltCashu,
    ObserveNutzapReceipts,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WalletCapabilities {
    pub pay_bolt11: bool,
    pub create_cashu_wallet: bool,
    pub publish_nutzap_info: bool,
    pub send_nutzap: bool,
    pub redeem_nutzap: bool,
    pub deposit_cashu: bool,
    pub melt_cashu: bool,
    pub observe_nutzap_receipts: bool,
}

impl WalletCapabilities {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            pay_bolt11: false,
            create_cashu_wallet: false,
            publish_nutzap_info: false,
            send_nutzap: false,
            redeem_nutzap: false,
            deposit_cashu: false,
            melt_cashu: false,
            observe_nutzap_receipts: false,
        }
    }

    #[must_use]
    pub const fn nwc_payments() -> Self {
        Self {
            pay_bolt11: true,
            ..Self::none()
        }
    }

    #[must_use]
    pub const fn cashu_nutzaps() -> Self {
        Self {
            create_cashu_wallet: true,
            publish_nutzap_info: true,
            send_nutzap: true,
            redeem_nutzap: true,
            deposit_cashu: true,
            observe_nutzap_receipts: true,
            ..Self::none()
        }
    }

    /// W2 (#2895) — the Cashu `WalletBackend` adapter's scope BEFORE #2917
    /// (W8/W9/W13) implemented nutzap send/receive/publish-info: create a
    /// wallet and deposit into it, nothing else. `CashuWalletBackend` itself
    /// has since moved on to `cashu_nutzaps()`; this constant survives as a
    /// smaller capability-set fixture other tests build stub backends from
    /// (see `selector_tests.rs`), not as a description of the real backend's
    /// current scope. Bundling nutzap flags into a backend that cannot
    /// execute them would advertise capabilities `start_intent` would
    /// silently no-op — the opposite of "absent capability means absent user
    /// action" — which is what kept this narrower constant around as a
    /// deliberately-incomplete fixture shape.
    #[must_use]
    pub const fn cashu_wallet_and_deposit() -> Self {
        Self {
            create_cashu_wallet: true,
            deposit_cashu: true,
            ..Self::none()
        }
    }

    #[must_use]
    pub fn supports(self, capability: WalletCapability) -> bool {
        match capability {
            WalletCapability::PayBolt11 => self.pay_bolt11,
            WalletCapability::CreateCashuWallet => self.create_cashu_wallet,
            WalletCapability::PublishNutzapInfo => self.publish_nutzap_info,
            WalletCapability::SendNutzap => self.send_nutzap,
            WalletCapability::RedeemNutzap => self.redeem_nutzap,
            WalletCapability::DepositCashu => self.deposit_cashu,
            WalletCapability::MeltCashu => self.melt_cashu,
            WalletCapability::ObserveNutzapReceipts => self.observe_nutzap_receipts,
        }
    }

    #[must_use]
    pub fn action_namespaces(self) -> Vec<&'static str> {
        let mut actions = Vec::new();
        if self.pay_bolt11 {
            actions.extend([
                ACTION_NWC_CONNECT,
                ACTION_NWC_DISCONNECT,
                ACTION_PAY_INVOICE,
            ]);
        }
        if self.create_cashu_wallet {
            actions.extend([ACTION_CASHU_CREATE, ACTION_CASHU_RECOVER]);
        }
        if self.deposit_cashu {
            actions.extend([ACTION_CASHU_DEPOSIT_QUOTE, ACTION_CASHU_COMPLETE_DEPOSIT]);
        }
        if self.publish_nutzap_info {
            actions.push(ACTION_NUTZAP_PUBLISH_INFO);
        }
        if self.send_nutzap {
            actions.push(ACTION_NUTZAP_SEND);
        }
        if self.redeem_nutzap {
            actions.push(ACTION_NUTZAP_REDEEM);
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_capability_means_absent_user_action() {
        let actions = WalletCapabilities::none().action_namespaces();
        assert!(actions.is_empty());

        let actions = WalletCapabilities::nwc_payments().action_namespaces();
        assert!(actions.contains(&ACTION_PAY_INVOICE));
        assert!(!actions.contains(&ACTION_NUTZAP_SEND));
    }

    #[test]
    fn cashu_actions_are_exposed_only_when_capabilities_exist() {
        let actions = WalletCapabilities::cashu_nutzaps().action_namespaces();
        assert!(actions.contains(&ACTION_CASHU_CREATE));
        assert!(actions.contains(&ACTION_NUTZAP_SEND));
        assert!(actions.contains(&ACTION_NUTZAP_REDEEM));
        assert!(!actions.contains(&ACTION_PAY_INVOICE));
    }

    /// W2 (#2895) — this narrower constant (pre-#2917) still advertises
    /// exactly create+deposit, not the full nutzap bundle — a stub-backend
    /// fixture shape `selector_tests.rs` still relies on; the REAL
    /// `CashuWalletBackend` advertises `cashu_nutzaps()` since #2917.
    #[test]
    fn cashu_wallet_and_deposit_advertises_only_create_and_deposit() {
        let caps = WalletCapabilities::cashu_wallet_and_deposit();
        assert!(caps.create_cashu_wallet);
        assert!(caps.deposit_cashu);
        assert!(!caps.pay_bolt11);
        assert!(!caps.publish_nutzap_info);
        assert!(!caps.send_nutzap);
        assert!(!caps.redeem_nutzap);
        assert!(!caps.melt_cashu);
        assert!(!caps.observe_nutzap_receipts);

        let actions = caps.action_namespaces();
        assert!(actions.contains(&ACTION_CASHU_CREATE));
        assert!(actions.contains(&ACTION_CASHU_DEPOSIT_QUOTE));
        assert!(actions.contains(&ACTION_CASHU_COMPLETE_DEPOSIT));
        assert!(!actions.contains(&ACTION_NUTZAP_SEND));
    }
}
