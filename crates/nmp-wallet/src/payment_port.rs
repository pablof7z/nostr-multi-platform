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

pub trait WalletPaymentCommandFactory: Send + Sync + std::fmt::Debug {
    fn pay_bolt11(&self, payment: WalletBolt11Payment) -> ActorCommand;
}

#[derive(Clone)]
pub struct WalletPaymentPort {
    factory: Arc<dyn WalletPaymentCommandFactory>,
}

impl WalletPaymentPort {
    #[must_use]
    pub fn new(factory: Arc<dyn WalletPaymentCommandFactory>) -> Self {
        Self { factory }
    }
}

impl std::fmt::Debug for WalletPaymentPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WalletPaymentPort")
    }
}

impl PaymentPort for WalletPaymentPort {
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

    impl WalletPaymentCommandFactory for RecordingFactory {
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
        let port_factory: Arc<dyn WalletPaymentCommandFactory> = factory.clone();
        let port = WalletPaymentPort::new(port_factory);

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
