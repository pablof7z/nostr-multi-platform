//! `ZapsView` — aggregate zap receipts targeting a specific event.
//!
//! Spec target is an event id (not an addressable coord); the view filters
//! receipts whose `e` tag matches. Addressable-target aggregation would need
//! a sibling `ZapsByAddressView`, intentionally out of scope here.

use std::collections::BTreeMap;

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
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
    /// `None` for anonymous receipts (NIP-57 permits a zap with no
    /// discoverable sender). The entry is still counted in `zap_count`,
    /// summed into `total_msats`, and present in `zappers` — the UI renders
    /// such a row as an anonymous zapper (D1: a `None` here is the truthful
    /// answer, not a not-yet-loaded placeholder).
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

impl ZapsView {
    pub const NAMESPACE: &'static str = "nmp.nip57.zaps";

    pub fn key(spec: &ZapsSpec) -> EventId {
        spec.target.clone()
    }

    pub fn dependencies(spec: &ZapsSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: vec![KIND_ZAP_RECEIPT],
            tag_refs: vec![("e".into(), spec.target.clone())],
            ..Default::default()
        }
    }

    pub fn open(_ctx: &ViewContext, spec: ZapsSpec) -> (ZapsState, ZapsPayload) {
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

    pub fn on_event_inserted(
        _c: &ViewContext,
        s: &mut ZapsState,
        e: &KernelEvent,
    ) -> Option<ZapsDelta> {
        s.insert(e)
    }

    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut ZapsState,
        id: &EventId,
    ) -> Option<ZapsDelta> {
        s.remove(id)
    }

    pub fn on_event_replaced(
        _c: &ViewContext,
        s: &mut ZapsState,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<ZapsDelta> {
        // Treat replace as remove+insert. Receipts aren't replaceable per the
        // spec but a relay-side replay shouldn't break us.
        let _ = s.remove(old);
        s.insert(e)
    }

    pub fn snapshot(_c: &ViewContext, state: &ZapsState) -> ZapsPayload {
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

    #[test]
    fn multiple_distinct_senders_each_appear_in_zappers() {
        // Three receipts to the same note from three distinct senders: the
        // total sums and every sender is represented in `zappers`.
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z1", "NOTE", 10_000, Some("alice")));
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z2", "NOTE", 20_000, Some("bob")));
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z3", "NOTE", 30_000, Some("carol")));
        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.zap_count, 3);
        assert_eq!(snap.total_msats, 10_000 + 20_000 + 30_000);
        let mut senders: Vec<&str> = snap
            .zappers
            .iter()
            .filter_map(|z| z.pubkey.as_deref())
            .collect();
        senders.sort_unstable();
        assert_eq!(senders, vec!["alice", "bob", "carol"]);
    }

    #[test]
    fn receipt_with_no_amount_contributes_zero_to_total() {
        // A receipt whose bolt11 has no parseable amount and no embedded
        // request defaults `msats` to 0 (see `unwrap_or(0)`); it still counts
        // as a zap but does not move the total.
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        let no_amount = KernelEvent {
            id: "ZN".into(),
            author: "ln_node".into(),
            kind: 9735,
            created_at: 1,
            tags: vec![
                vec!["p".into(), "recipient".into()],
                vec!["e".into(), "NOTE".into()],
            ],
            content: String::new(),
        };
        ZapsView::on_event_inserted(&ctx(), &mut state, &no_amount);
        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.zap_count, 1);
        assert_eq!(snap.total_msats, 0);
        assert_eq!(snap.zappers[0].msats, 0);
    }

    #[test]
    fn removing_a_receipt_drops_it_from_the_aggregate() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z1", "NOTE", 10_000, Some("alice")));
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z2", "NOTE", 20_000, Some("bob")));

        let delta = ZapsView::on_event_removed(&ctx(), &mut state, &"Z1".to_string());
        assert_eq!(delta, Some(ZapsDelta::Removed { receipt_id: "Z1".into() }));

        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.zap_count, 1);
        assert_eq!(snap.total_msats, 20_000);
        assert_eq!(snap.zappers[0].receipt_id, "Z2");
    }

    #[test]
    fn removing_an_unknown_receipt_is_a_noop() {
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("Z1", "NOTE", 10_000, Some("alice")));
        let delta = ZapsView::on_event_removed(&ctx(), &mut state, &"NOT_PRESENT".to_string());
        assert!(delta.is_none());
        assert_eq!(ZapsView::snapshot(&ctx(), &state).zap_count, 1);
    }

    #[test]
    fn replacing_a_receipt_swaps_it_for_the_new_one() {
        // `on_event_replaced` is remove(old) + insert(new). A relay replay that
        // re-delivers a corrected receipt must end with exactly the new entry.
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("OLD", "NOTE", 10_000, Some("alice")));

        let new = receipt("NEW", "NOTE", 50_000, Some("alice"));
        let delta = ZapsView::on_event_replaced(&ctx(), &mut state, &"OLD".to_string(), &new);
        assert!(matches!(delta, Some(ZapsDelta::Added(_))));

        let snap = ZapsView::snapshot(&ctx(), &state);
        assert_eq!(snap.zap_count, 1);
        assert_eq!(snap.total_msats, 50_000);
        assert_eq!(snap.zappers[0].receipt_id, "NEW");
    }

    #[test]
    fn replace_with_receipt_for_other_target_just_removes_old() {
        // If the replacement points at a different note, the remove(old) still
        // lands but the insert is rejected — net effect is a pure removal.
        let spec = ZapsSpec { target: "NOTE".into() };
        let (mut state, _) = ZapsView::open(&ctx(), spec);
        ZapsView::on_event_inserted(&ctx(), &mut state, &receipt("OLD", "NOTE", 10_000, Some("alice")));

        let elsewhere = receipt("NEW", "OTHER_NOTE", 50_000, Some("alice"));
        let delta = ZapsView::on_event_replaced(&ctx(), &mut state, &"OLD".to_string(), &elsewhere);
        assert!(delta.is_none());
        assert_eq!(ZapsView::snapshot(&ctx(), &state).zap_count, 0);
    }
}
