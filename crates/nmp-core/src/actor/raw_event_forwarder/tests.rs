use std::sync::{Arc, Mutex};

use crate::actor::raw_event_forwarder::{
    new_raw_event_forward_observer_id_slot, register_raw_event_forward_policies,
    RawEventForwardObserver, RawEventForwardSender,
};
use crate::actor::{new_raw_event_observer_slot, raw_observers_idle_for_kind};
use crate::kernel::Kernel;
use crate::store::RawEvent;
use crate::substrate::{RawEventForwardPolicy, RawEventForwardTarget};
use crate::{KindFilter, RelayRole};

#[derive(Clone, Default)]
struct CaptureSender {
    sends: Arc<Mutex<Vec<(String, String, RelayRole)>>>,
}

impl CaptureSender {
    fn sends(&self) -> Vec<(String, String, RelayRole)> {
        self.sends.lock().expect("sends").clone()
    }
}

impl RawEventForwardSender for CaptureSender {
    fn send_to(&self, target: &RawEventForwardTarget, frame_text: &str) -> bool {
        self.sends.lock().expect("sends").push((
            target.relay_url.clone(),
            frame_text.to_string(),
            target.relay_role,
        ));
        true
    }
}

struct StaticPolicy {
    target: String,
}

impl RawEventForwardPolicy for StaticPolicy {
    fn kind_filter(&self) -> KindFilter {
        KindFilter::from_kinds([0u32])
    }

    fn forward_targets(
        &self,
        _raw: &RawEvent,
        _source_relay_url: Option<&str>,
    ) -> Vec<RawEventForwardTarget> {
        vec![RawEventForwardTarget::new(
            self.target.clone(),
            RelayRole::Indexer,
        )]
    }
}

fn make_raw(kind: u32) -> RawEvent {
    RawEvent {
        id: "01".repeat(32),
        pubkey: "11".repeat(32),
        created_at: 1_700_000_000,
        kind,
        tags: Vec::new(),
        content: String::new(),
        sig: "22".repeat(64),
    }
}

#[test]
fn observer_builds_event_frame_and_sends_to_policy_targets() {
    let sender = Arc::new(CaptureSender::default());
    let observer = RawEventForwardObserver::new(
        Arc::new(StaticPolicy {
            target: "wss://indexer/".into(),
        }),
        Arc::clone(&sender) as Arc<dyn RawEventForwardSender>,
    );
    let raw = make_raw(0);
    let json = serde_json::to_string(&raw).expect("raw json");

    let sent = observer.process(&raw, Some("wss://content/"), &json);

    assert_eq!(sent, 1);
    let sends = sender.sends();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].0, "wss://indexer/");
    assert_eq!(sends[0].2, RelayRole::Indexer);
    assert!(sends[0].1.starts_with(r#"["EVENT","#));
    assert!(sends[0].1.ends_with(']'));
}

#[test]
fn re_register_unregisters_stale_policy_observers() {
    let raw_slot = new_raw_event_observer_slot();
    let id_slot = new_raw_event_forward_observer_id_slot();
    let policy_slot = crate::slots::new_raw_event_forward_policy_slot();
    {
        let mut guard = policy_slot.lock().expect("policy slot");
        *guard = Some(Arc::new(|_context| {
            vec![Arc::new(StaticPolicy {
                target: "wss://indexer/".into(),
            }) as Arc<dyn RawEventForwardPolicy>]
        }));
    }
    let (relay_tx, _relay_rx) = std::sync::mpsc::channel();
    let pool = nmp_network::pool::Pool::new(nmp_network::pool::PoolConfig::default(), relay_tx);
    let kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);

    register_raw_event_forward_policies(&kernel, &raw_slot, &pool, &id_slot, &policy_slot);
    assert!(!raw_observers_idle_for_kind(&raw_slot, 0));
    assert_eq!(id_slot.lock().expect("id slot").len(), 1);
    let first_id = id_slot.lock().expect("id slot")[0];

    register_raw_event_forward_policies(&kernel, &raw_slot, &pool, &id_slot, &policy_slot);

    assert!(!raw_observers_idle_for_kind(&raw_slot, 0));
    let ids = id_slot.lock().expect("id slot").clone();
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], first_id);
}
