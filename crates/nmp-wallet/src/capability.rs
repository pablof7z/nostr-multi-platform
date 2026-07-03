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
}
