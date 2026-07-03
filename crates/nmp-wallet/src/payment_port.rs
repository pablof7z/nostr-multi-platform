use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{PaymentIntent, PaymentPort};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletBolt11Payment {
    pub bolt11: String,
    pub amount_msats: Option<u64>,
    pub correlation_id: Option<String>,
}

impl From<PaymentIntent> for WalletBolt11Payment {
    fn from(intent: PaymentIntent) -> Self {
        Self {
            bolt11: intent.bolt11,
            amount_msats: intent.amount_msats,
            correlation_id: intent.correlation_id,
        }
    }
}

pub trait WalletBackendPaymentCommandFactory: Send + Sync + std::fmt::Debug {
    fn pay_bolt11(&self, payment: WalletBolt11Payment) -> ActorCommand;
}

/// Routes payment intents to whichever backend `nmp-wallet` has selected.
///
/// This implements `PaymentPort` so it can be handed to
/// `nmp_nip57::Config::with_payment_port`, but it is a selection indirection
/// in front of the real backend adapters, not a competing protocol adapter:
/// each backend still owns its own `PaymentPort` implementation
/// (`nmp_nip47::WalletPaymentPort` for NWC today) and this type just forwards
/// to whichever one `nmp-wallet` has selected. Phase-1 item W11 (#2864)
/// decides whether this indirection is kept at all, or whether backend
/// selection instead passes the selected backend's `Arc<dyn PaymentPort>`
/// straight through to `nmp_nip57::Config::with_payment_port`.
#[derive(Clone)]
pub struct WalletBackendPaymentRouter {
    factory: Arc<dyn WalletBackendPaymentCommandFactory>,
}

impl WalletBackendPaymentRouter {
    #[must_use]
    pub fn new(factory: Arc<dyn WalletBackendPaymentCommandFactory>) -> Self {
        Self { factory }
    }
}

impl std::fmt::Debug for WalletBackendPaymentRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WalletBackendPaymentRouter")
    }
}

impl PaymentPort for WalletBackendPaymentRouter {
    fn pay_invoice(&self, intent: PaymentIntent) -> ActorCommand {
        self.factory.pay_bolt11(intent.into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingFactory {
        seen: Mutex<Vec<WalletBolt11Payment>>,
    }

    impl WalletBackendPaymentCommandFactory for RecordingFactory {
        fn pay_bolt11(&self, payment: WalletBolt11Payment) -> ActorCommand {
            self.seen.lock().unwrap().push(payment);
            ActorCommand::ShowToast {
                message: "pay".to_string(),
            }
        }
    }

    #[test]
    fn payment_port_delegates_invoice_intent_to_wallet_factory() {
        let factory = Arc::new(RecordingFactory::default());
        let port_factory: Arc<dyn WalletBackendPaymentCommandFactory> = factory.clone();
        let port = WalletBackendPaymentRouter::new(port_factory);

        let command = port.pay_invoice(PaymentIntent {
            bolt11: "lnbc1".to_string(),
            amount_msats: Some(21_000),
            correlation_id: Some("corr".to_string()),
        });

        assert!(matches!(command, ActorCommand::ShowToast { .. }));
        let seen = factory.seen.lock().unwrap();
        assert_eq!(
            seen.as_slice(),
            [WalletBolt11Payment {
                bolt11: "lnbc1".to_string(),
                amount_msats: Some(21_000),
                correlation_id: Some("corr".to_string()),
            }]
        );
    }
}
