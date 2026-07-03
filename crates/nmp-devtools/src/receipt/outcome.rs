use serde::{Deserialize, Serialize};

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
        Self::new(XrayOutcomeStatus::Applied, "applied")
    }

    #[must_use]
    pub fn retained() -> Self {
        Self::new(XrayOutcomeStatus::Retained, "retained")
    }

    #[must_use]
    pub fn pending(code: impl Into<String>) -> Self {
        Self::new(XrayOutcomeStatus::Pending, code)
    }

    #[must_use]
    pub fn failed(code: impl Into<String>) -> Self {
        Self::new(XrayOutcomeStatus::Failed, code)
    }

    #[must_use]
    pub fn unknown(code: impl Into<String>) -> Self {
        Self::new(XrayOutcomeStatus::Unknown, code)
    }

    #[must_use]
    pub fn new(status: XrayOutcomeStatus, code: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
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

    #[must_use]
    pub fn with_outcome(mut self, outcome: XrayCommandOutcome) -> Self {
        self.outcome = outcome;
        self
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
