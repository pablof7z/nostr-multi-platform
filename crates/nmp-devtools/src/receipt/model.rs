use serde::{Deserialize, Serialize};

use super::{XrayCauseLink, XrayReason};

/// Wall-clock timestamp assigned by NMP at the receipt boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayTimestamp {
    pub unix_ms: u64,
}

impl XrayTimestamp {
    #[must_use]
    pub const fn new(unix_ms: u64) -> Self {
        Self { unix_ms }
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

/// Stable context shared by receipts emitted for one projection transaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayProjectionContext {
    pub projection_key: String,
    pub view_label: String,
    pub parent_scope: Option<String>,
    pub owner_key: String,
    pub reason: XrayReason,
}

impl XrayProjectionContext {
    #[must_use]
    pub fn new(
        projection_key: impl Into<String>,
        view_label: impl Into<String>,
        owner_key: impl Into<String>,
        reason: XrayReason,
    ) -> Self {
        Self {
            projection_key: projection_key.into(),
            view_label: view_label.into(),
            parent_scope: None,
            owner_key: owner_key.into(),
            reason,
        }
    }

    #[must_use]
    pub fn with_parent_scope(mut self, parent_scope: impl Into<String>) -> Self {
        self.parent_scope = Some(parent_scope.into());
        self
    }
}

/// NMP-owned description of the interest affected by a resource receipt.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayInterestDescriptor {
    pub interest_key: String,
    pub scope: String,
    pub shape: String,
    pub provenance: String,
    pub privacy_bearing: bool,
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
            privacy_bearing: true,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayOutcomeStatus {
    Applied,
    Retained,
    Pending,
    Failed,
    Unknown,
}

/// Outcome after a receipt is joined to kernel/socket effects.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayCommandOutcome {
    pub status: XrayOutcomeStatus,
    pub code: String,
}

impl XrayCommandOutcome {
    #[must_use]
    pub fn applied() -> Self {
        Self {
            status: XrayOutcomeStatus::Applied,
            code: "applied".to_string(),
        }
    }
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
    pub outcome: XrayCommandOutcome,
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
            outcome: XrayCommandOutcome::applied(),
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
    pub timestamp: XrayTimestamp,
    pub context: XrayProjectionContext,
    pub event: XrayReceiptEventKind,
    pub resource_id: String,
    pub interest: Option<XrayInterestDescriptor>,
    pub owner_counts: XrayOwnerCounts,
    pub outcome: XrayCommandOutcome,
    pub teardown: XrayTeardownCascade,
    pub relay_effects: Vec<XrayRelayEffect>,
    pub cause: Option<XrayCauseLink>,
}

impl XrayReceipt {
    #[must_use]
    pub fn new(
        context: XrayProjectionContext,
        transaction: XrayTransactionMarker,
        timestamp: XrayTimestamp,
        event: XrayReceiptEventKind,
        resource_id: impl Into<String>,
        interest: Option<XrayInterestDescriptor>,
    ) -> Self {
        Self {
            sequence: 0,
            transaction,
            timestamp,
            context,
            event,
            resource_id: resource_id.into(),
            interest,
            owner_counts: XrayOwnerCounts::unknown(),
            outcome: XrayCommandOutcome::applied(),
            teardown: XrayTeardownCascade::default(),
            relay_effects: Vec::new(),
            cause: None,
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

    #[must_use]
    pub fn with_cause(mut self, cause: XrayCauseLink) -> Self {
        self.cause = Some(cause);
        self
    }
}
