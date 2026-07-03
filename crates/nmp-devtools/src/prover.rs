use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{XrayCapsule, XrayReceipt, XrayReceiptEventKind};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayScopeInventory {
    pub scope: String,
    pub owner_keys: Vec<String>,
    pub open_resources: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayReplayTransaction {
    pub transaction: u64,
    pub revision: u64,
    pub receipts: Vec<XrayReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayReplaySession {
    pub transactions: Vec<XrayReplayTransaction>,
}

#[derive(Clone, Debug)]
pub struct XrayProbe {
    receipts: Vec<XrayReceipt>,
}

impl XrayProbe {
    #[must_use]
    pub fn new(receipts: Vec<XrayReceipt>) -> Self {
        Self { receipts }
    }

    #[must_use]
    pub fn from_capsule(capsule: &XrayCapsule) -> Self {
        Self::new(capsule.receipts.clone())
    }

    #[must_use]
    pub fn why_subscription_open(&self, key: &str) -> Vec<XrayReceipt> {
        self.receipts
            .iter()
            .filter(|receipt| {
                matches!(
                    receipt.event,
                    XrayReceiptEventKind::Open
                        | XrayReceiptEventKind::Replace
                        | XrayReceiptEventKind::Refresh
                ) && receipt_matches_key(receipt, key)
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn why_view_stale(&self, view: &str) -> Vec<XrayReceipt> {
        self.receipts
            .iter()
            .filter(|receipt| {
                receipt.context.projection_key == view || receipt.context.view_label == view
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn what_closed_this_relay(&self, relay_url: &str) -> Vec<XrayReceipt> {
        self.receipts
            .iter()
            .filter(|receipt| {
                receipt.event == XrayReceiptEventKind::Close
                    && receipt
                        .relay_effects
                        .iter()
                        .any(|effect| effect.relay_url == relay_url)
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn list_scope_inventory(&self, scope: &str) -> XrayScopeInventory {
        let mut owner_keys = BTreeSet::new();
        let mut open_resources = BTreeSet::new();
        for receipt in self.receipts.iter().filter(|receipt| {
            receipt.context.view_label == scope
                || receipt.context.parent_scope.as_deref() == Some(scope)
                || receipt
                    .interest
                    .as_ref()
                    .is_some_and(|interest| interest.scope == scope)
        }) {
            owner_keys.insert(receipt.context.owner_key.clone());
            match receipt.event {
                XrayReceiptEventKind::Open
                | XrayReceiptEventKind::Replace
                | XrayReceiptEventKind::Refresh => {
                    open_resources.insert(receipt.resource_id.clone());
                }
                XrayReceiptEventKind::Close => {
                    open_resources.remove(&receipt.resource_id);
                }
            }
        }
        XrayScopeInventory {
            scope: scope.to_string(),
            owner_keys: owner_keys.into_iter().collect(),
            open_resources: open_resources.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn replay_session(capsule: &XrayCapsule) -> XrayReplaySession {
        let mut grouped: BTreeMap<(u64, u64), Vec<XrayReceipt>> = BTreeMap::new();
        for receipt in &capsule.receipts {
            grouped
                .entry((
                    receipt.transaction.transaction,
                    receipt.transaction.revision,
                ))
                .or_default()
                .push(receipt.clone());
        }
        let transactions = grouped
            .into_iter()
            .map(
                |((transaction, revision), receipts)| XrayReplayTransaction {
                    transaction,
                    revision,
                    receipts,
                },
            )
            .collect();
        XrayReplaySession { transactions }
    }
}

fn receipt_matches_key(receipt: &XrayReceipt, key: &str) -> bool {
    receipt.resource_id == key
        || receipt
            .interest
            .as_ref()
            .is_some_and(|interest| interest.interest_key == key)
}

#[cfg(test)]
mod tests {
    use crate::{
        XrayCapsule, XrayCapsuleProducer, XrayInterestDescriptor, XrayProjectionContext,
        XrayReason, XrayReasonCode, XrayReceiptEventKind, XrayRedactionMode, XrayRelayEffect,
        XraySymbolicationManifest, XrayTimestamp, XrayTransactionMarker,
    };

    use super::*;

    fn receipt(event: XrayReceiptEventKind, resource: &str) -> XrayReceipt {
        XrayReceipt::new(
            XrayProjectionContext::new(
                "chirp.home",
                "home",
                "owner",
                XrayReason::new(XrayReasonCode::FeedSessionSync),
            ),
            XrayTransactionMarker::new(9, 1),
            XrayTimestamp::new(10),
            event,
            resource,
            Some(XrayInterestDescriptor::new(
                "interest-a",
                "home",
                "authors=[a]",
                "active-follow-timeline",
            )),
        )
        .with_relay_effects(vec![XrayRelayEffect::new("relay-a", None, "closed", 0, 0)])
    }

    #[test]
    fn probe_answers_receipt_queries() {
        let probe = XrayProbe::new(vec![
            receipt(XrayReceiptEventKind::Open, "resource-a"),
            receipt(XrayReceiptEventKind::Close, "resource-a"),
        ]);

        assert_eq!(probe.why_subscription_open("resource-a").len(), 1);
        assert_eq!(probe.what_closed_this_relay("relay-a").len(), 1);
        assert!(probe.list_scope_inventory("home").open_resources.is_empty());
    }

    #[test]
    fn replay_groups_receipts_by_transaction() {
        let capsule = XrayCapsule::new(
            XrayCapsuleProducer {
                app: "chirp".into(),
                nmp_version: "0.8.4".into(),
                trellis_version: None,
                platform: "test".into(),
                recording_started_unix_ms: 1,
                recording_ended_unix_ms: 2,
            },
            XrayRedactionMode::LocalDebug,
            XraySymbolicationManifest::default(),
            vec![receipt(XrayReceiptEventKind::Open, "resource-a")],
        );

        let replay = XrayProbe::replay_session(&capsule);
        assert_eq!(replay.transactions.len(), 1);
        assert_eq!(replay.transactions[0].transaction, 9);
    }
}
