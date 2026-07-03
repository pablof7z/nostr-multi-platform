use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSessionDiagnosticBatch {
    pub projection_key: String,
    pub view_label: String,
    pub parent_scope: Option<String>,
    pub owner_key: String,
    pub transaction: FeedSessionDiagnosticTransaction,
    pub reason: FeedSessionDiagnosticReason,
    pub receipts: Vec<FeedSessionDiagnosticReceipt>,
}

impl FeedSessionDiagnosticBatch {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FeedSessionDiagnosticContext {
    pub(crate) projection_key: String,
    pub(crate) view_label: String,
    pub(crate) parent_scope: Option<String>,
    pub(crate) owner_key: String,
}

impl FeedSessionDiagnosticContext {
    #[must_use]
    pub(crate) fn new(
        projection_key: impl Into<String>,
        view_label: impl Into<String>,
        parent_scope: Option<String>,
        owner_key: impl Into<String>,
    ) -> Self {
        Self {
            projection_key: projection_key.into(),
            view_label: view_label.into(),
            parent_scope,
            owner_key: owner_key.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FeedSessionDiagnosticTransaction {
    pub transaction: u64,
    pub revision: u64,
}

impl FeedSessionDiagnosticTransaction {
    #[must_use]
    pub const fn new(transaction: u64, revision: u64) -> Self {
        Self {
            transaction,
            revision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSessionDiagnosticReason {
    pub code: FeedSessionDiagnosticReasonCode,
    pub label: String,
}

impl FeedSessionDiagnosticReason {
    #[must_use]
    pub fn new(code: FeedSessionDiagnosticReasonCode, label: impl Into<String>) -> Self {
        Self {
            code,
            label: label.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedSessionDiagnosticReasonCode {
    AcquisitionSync,
    SourceEffect,
    AcquisitionClose,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSessionDiagnosticReceipt {
    pub event: FeedSessionDiagnosticEventKind,
    pub resource_id: String,
    pub interest: Option<FeedSessionDiagnosticInterest>,
    pub owner_counts: FeedSessionDiagnosticOwnerCounts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedSessionDiagnosticEventKind {
    Open,
    Replace,
    Refresh,
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedSessionDiagnosticInterest {
    pub interest_key: String,
    pub scope: String,
    pub shape: String,
    pub provenance: String,
    pub privacy_bearing: bool,
}

impl FeedSessionDiagnosticInterest {
    #[must_use]
    pub(crate) fn new(
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FeedSessionDiagnosticOwnerCounts {
    pub before: u32,
    pub after: u32,
}

impl FeedSessionDiagnosticOwnerCounts {
    #[must_use]
    pub const fn known(before: u32, after: u32) -> Self {
        Self { before, after }
    }
}

pub trait FeedSessionDiagnosticsSink: Send + Sync {
    fn is_enabled(&self) -> bool {
        true
    }

    fn record(&self, batch: FeedSessionDiagnosticBatch);
}

#[derive(Clone, Default)]
pub struct FeedSessionDiagnosticsHandle {
    sink: Option<Arc<dyn FeedSessionDiagnosticsSink>>,
}

impl FeedSessionDiagnosticsHandle {
    #[must_use]
    pub const fn disabled() -> Self {
        Self { sink: None }
    }

    #[must_use]
    pub fn new(sink: Arc<dyn FeedSessionDiagnosticsSink>) -> Self {
        Self { sink: Some(sink) }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.sink.as_ref().is_some_and(|sink| sink.is_enabled())
    }

    pub fn record(&self, batch: FeedSessionDiagnosticBatch) {
        let Some(sink) = &self.sink else {
            return;
        };
        if batch.is_empty() {
            return;
        }
        sink.record(batch);
    }
}
