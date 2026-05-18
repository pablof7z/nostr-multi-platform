//! `ZapsView` — aggregate zap receipts targeting a specific event.
//!
//! Spec target is an event id (not an addressable coord); the view filters
//! receipts whose `e` tag matches. Addressable-target aggregation would need
//! a sibling `ZapsByAddressView`, intentionally out of scope here.

use std::collections::BTreeMap;

use nmp_core::substrate::{
    EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule,
};
use serde::{Deserialize, Serialize};

use crate::decode::try_from_kernel_event;
use crate::kinds::KIND_ZAP_RECEIPT;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ZapsSpec {
    pub target: EventId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ZapEntry {
    /// Sender pubkey if known (uppercase `P` tag, else embedded request).
    /// Receipts without a discoverable sender contribute to `total_msats` but
    /// not to `zappers`.
    pub pubkey: Option<String>,
    pub msats: u64,
    /// Receipt event id — used as the dedupe key (NIP-57 doesn't forbid the
    /// same receipt being delivered more than once across relays).
    pub receipt_id: EventId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ZapsPayload {
    pub target_id: EventId,
    pub total_msats: u64,
    pub zap_count: u32,
    pub zappers: Vec<ZapEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ZapsDelta {
    Added(ZapEntry),
    Removed { receipt_id: EventId },
}

#[derive(Default)]
pub struct ZapsState {
    target: EventId,
    by_id: BTreeMap<EventId, ZapEntry>,
}

impl ZapsState {
    fn insert(&mut self, event: &KernelEvent) -> Option<ZapsDelta> {
        let record = try_from_kernel_event(event)?;
        if record.zapped_event_id.as_deref() != Some(self.target.as_str()) {
            return None;
        }
        if self.by_id.contains_key(&record.event_id) {
            return None;
        }
        let entry = ZapEntry {
            pubkey: record.sender_pubkey.clone(),
            msats: record.amount_msats.unwrap_or(0),
            receipt_id: record.event_id.clone(),
        };
        self.by_id.insert(entry.receipt_id.clone(), entry.clone());
        Some(ZapsDelta::Added(entry))
    }

    fn remove(&mut self, id: &EventId) -> Option<ZapsDelta> {
        self.by_id.remove(id)?;
        Some(ZapsDelta::Removed { receipt_id: id.clone() })
    }
}

pub struct ZapsView;

impl ViewModule for ZapsView {
    const NAMESPACE: &'static str = "nmp.nip57.zaps";
    type Spec = ZapsSpec;
    type Payload = ZapsPayload;
    type Delta = ZapsDelta;
    type Key = EventId;
    type State = ZapsState;

    fn key(spec: &Self::Spec) -> Self::Key {
        spec.target.clone()
    }

    fn dependencies(spec: &Self::Spec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_ZAP_RECEIPT],
            tag_refs: vec![("e".into(), spec.target.clone())],
            ..Default::default()
        }
    }

    fn open(_ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload) {
        let state = ZapsState {
            target: spec.target.clone(),
            ..ZapsState::default()
        };
        let payload = ZapsPayload {
            target_id: spec.target,
            total_msats: 0,
            zap_count: 0,
            zappers: Vec::new(),
        };
        (state, payload)
    }

    fn on_event_inserted(
        _c: &ViewContext,
        s: &mut Self::State,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        s.insert(e)
    }

    fn on_event_removed(
        _c: &ViewContext,
        s: &mut Self::State,
        id: &EventId,
    ) -> Option<Self::Delta> {
        s.remove(id)
    }

    fn on_event_replaced(
        _c: &ViewContext,
        s: &mut Self::State,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<Self::Delta> {
        // Treat replace as remove+insert. Receipts aren't replaceable per the
        // spec but a relay-side replay shouldn't break us.
        let _ = s.remove(old);
        s.insert(e)
    }

    fn on_projection_changed(
        _c: &ViewContext,
        _s: &mut Self::State,
        _ch: &ProjectionChange,
    ) -> Option<Self::Delta> {
        None
    }

    fn snapshot(_c: &ViewContext, state: &Self::State) -> Self::Payload {
        let total_msats = state.by_id.values().map(|e| e.msats).sum();
        let zap_count = state.by_id.len() as u32;
        let zappers: Vec<ZapEntry> = state.by_id.values().cloned().collect();
        ZapsPayload {
            target_id: state.target.clone(),
            total_msats,
            zap_count,
            zappers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ViewContext {
        ViewContext::default()
    }

    fn receipt(id: &str, target: &str, msats: u64, sender: Option<&str>) -> KernelEvent {
        let mut tags = vec![
            vec!["p".into(), "recipient".into()],
            vec!["e".into(), target.into()],
            vec!["bolt11".into(), format!("lnbc{}n1pvj...", msats / 100)],
        ];
        if let Some(s) = sender {
            tags.push(vec!["P".into(), s.into()]);
        }
        KernelEvent {
            id: id.into(),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1,
            tags,
            content: String::new(),
        }
    }

    #[test]
    fn aggregates_msats_across_receipts() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        // Two receipts: 150 sats + 300 sats = 450 sats = 450_000 msat total.
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z1", "NOTE", 15_000, Some("alice")));
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z2", "NOTE", 30_000, Some("bob")));
        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.target_id, "NOTE");
        assert_eq!(snap.zap_count, 2);
        assert_eq!(snap.total_msats, 15_000 + 30_000);
    }

    #[test]
    fn rejects_receipts_for_other_targets() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        let r = receipt("Z", "OTHER", 15_000, Some("alice"));
        assert!(ZapsView::on_event_inserted(&ctx(), &mut state, &r).is_none());
    }

    #[test]
    fn duplicate_receipt_id_is_ignored() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        let r = receipt("Z1", "NOTE", 15_000, Some("alice"));
        ZapsView::on_event_inserted(&ctx(), &mut state, &r);
        assert!(ZapsView::on_event_inserted(&ctx(), &mut state, &r).is_none());
        assert_eq!(ZapsView::snapshot(&ctx(), &state).zap_count, 1);
    }

    #[test]
    fn anonymous_zap_contributes_to_total_but_has_none_sender() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("ZA", "NOTE", 15_000, None));
        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.zap_count, 1);
        assert!(snap.zappers[0].pubkey.is_none());
        assert!(snap.total_msats > 0);
    }

    #[test]
    fn dependencies_advertises_e_tag_ref() {
        let spec = ZapsSpec { target: "TID".into() };
        let deps = ZapsView::dependencies(&spec);
        assert_eq!(deps.kinds, vec![KIND_ZAP_RECEIPT]);
        assert_eq!(deps.tag_refs, vec![("e".into(), "TID".into())]);
    }
}
