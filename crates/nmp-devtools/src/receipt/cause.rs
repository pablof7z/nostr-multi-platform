use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XrayReasonCode {
    FeedSessionSync,
    FeedSessionSourceEffect,
    FeedSessionAcquisitionClose,
    ScopeClosed,
    RelayStateChanged,
    ReplayLoaded,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayReasonParam {
    pub key: String,
    pub value: String,
}

impl XrayReasonParam {
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Structured cause code rendered to prose only at the tool/UI edge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayReason {
    pub code: XrayReasonCode,
    pub params: Vec<XrayReasonParam>,
}

impl XrayReason {
    #[must_use]
    pub fn new(code: XrayReasonCode) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_params(code: XrayReasonCode, params: Vec<XrayReasonParam>) -> Self {
        Self { code, params }
    }
}

/// Structured causal link used by CLI/MCP tools and the Chirp pane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct XrayCauseLink {
    pub id: String,
    pub parent_id: Option<String>,
    pub reason: XrayReason,
}

impl XrayCauseLink {
    #[must_use]
    pub fn root(id: impl Into<String>, reason: XrayReason) -> Self {
        Self {
            id: id.into(),
            parent_id: None,
            reason,
        }
    }
}
