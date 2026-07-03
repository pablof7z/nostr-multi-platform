use nmp_core::actor::ActorCommand;
use nmp_core::substrate::KernelEvent;
use serde::{Deserialize, Serialize};

use crate::{WalletCapabilities, WalletProjection};

pub mod nwc;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct WalletBackendId(String);

impl WalletBackendId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WalletProjectionScope {
    pub include_history: bool,
    pub include_receive_rows: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WalletBackendSnapshot {
    pub projection: WalletProjection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalletIntent {
    SelectBackend {
        backend_id: WalletBackendId,
    },
    PayBolt11 {
        bolt11: String,
        amount_msats: Option<u64>,
    },
    CreateCashuWallet {
        mint: String,
    },
    RecoverCashuWallet,
    PublishNutzapInfo,
    SendNutzap {
        recipient_pubkey: String,
        amount_sats: u64,
        target_event_id: Option<String>,
    },
    RedeemNutzap {
        event_id: String,
    },
    DepositCashu {
        amount_sats: u64,
    },
    MeltCashu {
        bolt11: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintResult {
    pub operation_id: String,
    pub status: MintResultStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MintResultStatus {
    Settled,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct WalletBackendContext<'a> {
    pub now_secs: u64,
    pub selected_backend: Option<&'a WalletBackendId>,
}

pub trait WalletBackend: Send + Sync {
    fn id(&self) -> WalletBackendId;
    fn capabilities(&self) -> WalletCapabilities;
    fn snapshot(&self, scope: WalletProjectionScope) -> WalletBackendSnapshot;
    fn start_intent(
        &self,
        ctx: WalletBackendContext<'_>,
        intent: WalletIntent,
        correlation_id: Option<String>,
    ) -> Vec<ActorCommand>;
    fn on_wallet_event(
        &self,
        ctx: WalletBackendContext<'_>,
        event: &KernelEvent,
    ) -> Vec<ActorCommand>;
    fn on_mint_result(
        &self,
        ctx: WalletBackendContext<'_>,
        result: MintResult,
    ) -> Vec<ActorCommand>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WalletProjection, WalletReadiness};

    struct EmptyBackend;

    impl WalletBackend for EmptyBackend {
        fn id(&self) -> WalletBackendId {
            WalletBackendId::new("empty")
        }

        fn capabilities(&self) -> WalletCapabilities {
            WalletCapabilities::none()
        }

        fn snapshot(&self, _scope: WalletProjectionScope) -> WalletBackendSnapshot {
            WalletBackendSnapshot {
                projection: WalletProjection::new(
                    Some(self.id()),
                    WalletReadiness::NotConfigured,
                    self.capabilities(),
                ),
            }
        }

        fn start_intent(
            &self,
            _ctx: WalletBackendContext<'_>,
            _intent: WalletIntent,
            _correlation_id: Option<String>,
        ) -> Vec<ActorCommand> {
            Vec::new()
        }

        fn on_wallet_event(
            &self,
            _ctx: WalletBackendContext<'_>,
            _event: &KernelEvent,
        ) -> Vec<ActorCommand> {
            Vec::new()
        }

        fn on_mint_result(
            &self,
            _ctx: WalletBackendContext<'_>,
            _result: MintResult,
        ) -> Vec<ActorCommand> {
            Vec::new()
        }
    }

    #[test]
    fn backend_snapshot_uses_wallet_projection_shape() {
        let backend = EmptyBackend;
        let snapshot = backend.snapshot(WalletProjectionScope::default());

        assert_eq!(
            snapshot
                .projection
                .active_backend_id
                .as_ref()
                .unwrap()
                .as_str(),
            "empty"
        );
        assert_eq!(snapshot.projection.capabilities, WalletCapabilities::none());
    }
}
