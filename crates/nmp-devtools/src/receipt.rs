use std::collections::VecDeque;
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

/// Stable context shared by receipts emitted for one projection transaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayProjectionContext {
    pub projection_key: String,
    pub scope_label: String,
    pub owner_key: String,
    pub reason: String,
}

impl XrayProjectionContext {
    #[must_use]
    pub fn new(
        projection_key: impl Into<String>,
        scope_label: impl Into<String>,
        owner_key: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            projection_key: projection_key.into(),
            scope_label: scope_label.into(),
            owner_key: owner_key.into(),
            reason: reason.into(),
        }
    }
}

/// Monotonic transaction coordinate assigned by the source reconciler.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayTransactionMarker {
    pub transaction: u64,
    pub revision: u64,
}

impl XrayTransactionMarker {
    #[must_use]
    pub const fn new(transaction: u64, revision: u64) -> Self {
        Self {
            transaction,
            revision,
        }
    }
}

/// NMP-owned description of the interest affected by a resource receipt.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayInterestDescriptor {
    pub interest_key: String,
    pub scope: String,
    pub shape: String,
    pub provenance: String,
}

impl XrayInterestDescriptor {
    #[must_use]
    pub fn new(
        interest_key: impl Into<String>,
        scope: impl Into<String>,
        shape: impl Into<String>,
        provenance: impl Into<String>,
    ) -> Self {
        Self {
            interest_key: interest_key.into(),
            scope: scope.into(),
            shape: shape.into(),
            provenance: provenance.into(),
        }
    }
}

/// Ordered resource event kind produced by a reconciler.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayReceiptEventKind {
    Open,
    Replace,
    Refresh,
    Close,
}

/// Owner-count state around a resource receipt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayOwnerCounts {
    pub before: Option<u32>,
    pub after: Option<u32>,
}

impl XrayOwnerCounts {
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            before: None,
            after: None,
        }
    }

    #[must_use]
    pub const fn known(before: u32, after: u32) -> Self {
        Self {
            before: Some(before),
            after: Some(after),
        }
    }
}

/// Relay or wire-subscription correlation attached to a receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayRelayEffect {
    pub relay_url: String,
    pub wire_id: Option<String>,
    pub state: String,
    pub consumer_count: u32,
    pub events_rx: u64,
}

impl XrayRelayEffect {
    #[must_use]
    pub fn new(
        relay_url: impl Into<String>,
        wire_id: Option<String>,
        state: impl Into<String>,
        consumer_count: u32,
        events_rx: u64,
    ) -> Self {
        Self {
            relay_url: relay_url.into(),
            wire_id,
            state: state.into(),
            consumer_count,
            events_rx,
        }
    }
}

/// Kernel teardown result attached after a receipt is joined to outcomes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayTeardownCascade {
    pub withdrawn_children: u32,
    pub closed_slots: u32,
    pub retained_owner_slots: u32,
}

impl XrayTeardownCascade {
    #[must_use]
    pub const fn new(
        withdrawn_children: u32,
        closed_slots: u32,
        retained_owner_slots: u32,
    ) -> Self {
        Self {
            withdrawn_children,
            closed_slots,
            retained_owner_slots,
        }
    }
}

/// One ordered X-Ray receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayReceipt {
    pub sequence: u64,
    pub transaction: XrayTransactionMarker,
    pub context: XrayProjectionContext,
    pub event: XrayReceiptEventKind,
    pub resource_key: String,
    pub interest: Option<XrayInterestDescriptor>,
    pub owner_counts: XrayOwnerCounts,
    pub teardown: XrayTeardownCascade,
    pub relay_effects: Vec<XrayRelayEffect>,
}

impl XrayReceipt {
    #[must_use]
    pub fn new(
        context: XrayProjectionContext,
        transaction: XrayTransactionMarker,
        event: XrayReceiptEventKind,
        resource_key: impl Into<String>,
        interest: Option<XrayInterestDescriptor>,
    ) -> Self {
        Self {
            sequence: 0,
            transaction,
            context,
            event,
            resource_key: resource_key.into(),
            interest,
            owner_counts: XrayOwnerCounts::unknown(),
            teardown: XrayTeardownCascade::default(),
            relay_effects: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_owner_counts(mut self, counts: XrayOwnerCounts) -> Self {
        self.owner_counts = counts;
        self
    }

    #[must_use]
    pub fn with_teardown(mut self, teardown: XrayTeardownCascade) -> Self {
        self.teardown = teardown;
        self
    }

    #[must_use]
    pub fn with_relay_effects(mut self, relay_effects: Vec<XrayRelayEffect>) -> Self {
        self.relay_effects = relay_effects;
        self
    }
}

/// Bounded ordered receipt stream.
#[derive(Clone, Debug)]
pub struct XrayReceiptStream {
    capacity: usize,
    next_sequence: u64,
    receipts: VecDeque<XrayReceipt>,
}

impl XrayReceiptStream {
    #[must_use]
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity: capacity.get(),
            next_sequence: 1,
            receipts: VecDeque::with_capacity(capacity.get()),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.receipts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }

    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn push(&mut self, mut receipt: XrayReceipt) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        receipt.sequence = sequence;
        self.receipts.push_back(receipt);
        while self.receipts.len() > self.capacity {
            self.receipts.pop_front();
        }
        sequence
    }

    pub fn push_batch<I>(&mut self, receipts: I)
    where
        I: IntoIterator<Item = XrayReceipt>,
    {
        for receipt in receipts {
            self.push(receipt);
        }
    }

    pub fn receipts(&self) -> impl ExactSizeIterator<Item = &XrayReceipt> {
        self.receipts.iter()
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<XrayReceipt> {
        self.receipts.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(key: &str) -> XrayReceipt {
        XrayReceipt::new(
            XrayProjectionContext::new("app.feed.home", "home", "owner", "test"),
            XrayTransactionMarker::new(7, 3),
            XrayReceiptEventKind::Open,
            key,
            None,
        )
    }

    #[test]
    fn stream_assigns_ordered_sequences_and_retains_bounded_tail() {
        let mut stream = XrayReceiptStream::new(NonZeroUsize::new(2).unwrap());

        assert_eq!(stream.push(receipt("a")), 1);
        assert_eq!(stream.push(receipt("b")), 2);
        assert_eq!(stream.push(receipt("c")), 3);

        let snapshot = stream.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].sequence, 2);
        assert_eq!(snapshot[0].resource_key, "b");
        assert_eq!(snapshot[1].sequence, 3);
        assert_eq!(snapshot[1].resource_key, "c");
        assert_eq!(stream.next_sequence(), 4);
    }
}
