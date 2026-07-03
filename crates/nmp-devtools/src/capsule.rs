use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::XrayReceipt;

pub const XRAY_CAPSULE_ENVELOPE_VERSION: u32 = 1;
pub const XRAY_RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayRedactionMode {
    LocalDebug,
    Shareable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayCapsuleVersions {
    pub envelope: u32,
    pub receipt_schema: u32,
    pub trellis_trace_format: Option<u32>,
}

impl XrayCapsuleVersions {
    #[must_use]
    pub const fn v1(trellis_trace_format: Option<u32>) -> Self {
        Self {
            envelope: XRAY_CAPSULE_ENVELOPE_VERSION,
            receipt_schema: XRAY_RECEIPT_SCHEMA_VERSION,
            trellis_trace_format,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayCapsuleProducer {
    pub app: String,
    pub nmp_version: String,
    pub trellis_version: Option<String>,
    pub platform: String,
    pub recording_started_unix_ms: u64,
    pub recording_ended_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XraySymbolicationEntry {
    pub opaque_id: String,
    pub label: String,
}

impl XraySymbolicationEntry {
    #[must_use]
    pub fn new(opaque_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            opaque_id: opaque_id.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XraySymbolicationManifest {
    pub nodes: Vec<XraySymbolicationEntry>,
    pub scopes: Vec<XraySymbolicationEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayCapsule {
    pub versions: XrayCapsuleVersions,
    pub producer: XrayCapsuleProducer,
    pub redaction: XrayRedactionMode,
    pub symbolication: XraySymbolicationManifest,
    pub receipts: Vec<XrayReceipt>,
}

impl XrayCapsule {
    #[must_use]
    pub fn new(
        producer: XrayCapsuleProducer,
        redaction: XrayRedactionMode,
        symbolication: XraySymbolicationManifest,
        receipts: Vec<XrayReceipt>,
    ) -> Self {
        let receipts = redact_receipts(receipts, redaction);
        Self {
            versions: XrayCapsuleVersions::v1(None),
            producer,
            redaction,
            symbolication,
            receipts,
        }
    }
}

#[must_use]
pub fn redact_receipts(
    receipts: Vec<XrayReceipt>,
    redaction: XrayRedactionMode,
) -> Vec<XrayReceipt> {
    match redaction {
        XrayRedactionMode::LocalDebug => receipts,
        XrayRedactionMode::Shareable => {
            let mut pseudonyms = Pseudonyms::default();
            receipts
                .into_iter()
                .map(|receipt| pseudonyms.redact_receipt(receipt))
                .collect()
        }
    }
}

#[derive(Default)]
struct Pseudonyms {
    next: BTreeMap<&'static str, usize>,
    seen: BTreeMap<(String, String), String>,
}

impl Pseudonyms {
    fn redact_receipt(&mut self, mut receipt: XrayReceipt) -> XrayReceipt {
        receipt.resource_id = self.token("resource", &receipt.resource_id);
        receipt.context.owner_key = self.token("owner", &receipt.context.owner_key);
        receipt.context.view_label = self.token("scope", &receipt.context.view_label);
        if let Some(parent) = &mut receipt.context.parent_scope {
            *parent = self.token("scope", parent);
        }
        if let Some(interest) = &mut receipt.interest {
            interest.interest_key = self.token("interest", &interest.interest_key);
            interest.scope = self.token("scope", &interest.scope);
            interest.shape = self.token("filter", &interest.shape);
            if let Some(wire_id) = &mut interest.wire_id_hint {
                *wire_id = self.token("wire", wire_id);
            }
            if let Some(interest_id) = &mut interest.planner_interest_id_hint {
                *interest_id = self.token("planner-interest", interest_id);
            }
        }
        for relay in &mut receipt.relay_effects {
            relay.relay_url = self.token("relay", &relay.relay_url);
            if let Some(wire_id) = &mut relay.wire_id {
                *wire_id = self.token("wire", wire_id);
            }
        }
        receipt
    }

    fn token(&mut self, kind: &'static str, value: &str) -> String {
        let key = (kind.to_string(), value.to_string());
        if let Some(existing) = self.seen.get(&key) {
            return existing.clone();
        }
        let next = self.next.entry(kind).or_insert(0);
        *next += 1;
        let token = format!("{kind}-{}", *next);
        self.seen.insert(key, token.clone());
        token
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        XrayInterestDescriptor, XrayProjectionContext, XrayReason, XrayReasonCode,
        XrayReceiptEventKind, XrayRelayEffect, XrayTimestamp, XrayTransactionMarker,
    };

    use super::*;

    fn producer() -> XrayCapsuleProducer {
        XrayCapsuleProducer {
            app: "chirp".to_string(),
            nmp_version: "0.8.4".to_string(),
            trellis_version: Some("0.2.1".to_string()),
            platform: "test".to_string(),
            recording_started_unix_ms: 10,
            recording_ended_unix_ms: 20,
        }
    }

    fn receipt(resource: &str, relay: &str) -> XrayReceipt {
        XrayReceipt::new(
            XrayProjectionContext::new(
                "chirp.feed",
                "home-feed",
                "owner:alice",
                XrayReason::new(XrayReasonCode::FeedSessionSync),
            ),
            XrayTransactionMarker::new(1, 2),
            XrayTimestamp::new(3),
            XrayReceiptEventKind::Open,
            resource,
            Some(XrayInterestDescriptor::new(
                "interest:alice",
                "active-account",
                "authors=[alice-pubkey];ids=[event-secret]",
                "active-follow-timeline",
            )),
        )
        .with_relay_effects(vec![XrayRelayEffect::new(
            relay,
            Some("wire-a".into()),
            "open",
            1,
            0,
        )])
    }

    #[test]
    fn shareable_redaction_is_stable_and_removes_private_values() {
        let capsule = XrayCapsule::new(
            producer(),
            XrayRedactionMode::Shareable,
            XraySymbolicationManifest::default(),
            vec![
                receipt("resource-secret", "wss://relay.example"),
                receipt("resource-secret", "wss://relay.example"),
            ],
        );
        let json = serde_json::to_string(&capsule).unwrap();

        assert!(!json.contains("resource-secret"));
        assert!(!json.contains("relay.example"));
        assert!(!json.contains("alice-pubkey"));
        assert_eq!(
            capsule.receipts[0].resource_id,
            capsule.receipts[1].resource_id
        );
        assert_eq!(
            capsule.receipts[0].relay_effects[0].relay_url,
            capsule.receipts[1].relay_effects[0].relay_url
        );
    }
}
