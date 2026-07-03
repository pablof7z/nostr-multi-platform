use std::sync::{Arc, Mutex};

use nmp_feed_session::{
    FeedSessionDiagnosticBatch, FeedSessionDiagnosticEventKind, FeedSessionDiagnosticInterest,
    FeedSessionDiagnosticReason, FeedSessionDiagnosticReasonCode, FeedSessionDiagnosticReceipt,
    FeedSessionDiagnosticsSink,
};

use crate::{
    XrayInterestDescriptor, XrayOwnerCounts, XrayProjectionContext, XrayReason, XrayReasonCode,
    XrayReasonParam, XrayReceipt, XrayReceiptEventKind, XrayReceiptRecorder, XrayRecordingConfig,
    XrayTimestamp, XrayTransactionMarker,
};

pub type XrayFeedSessionClock = Arc<dyn Fn() -> XrayTimestamp + Send + Sync>;

/// Private devtools sink that records live feed-session batches as X-Ray receipts.
pub struct XrayFeedSessionRecorder {
    recorder: Mutex<XrayReceiptRecorder>,
    clock: XrayFeedSessionClock,
}

impl XrayFeedSessionRecorder {
    #[must_use]
    pub fn enabled(config: XrayRecordingConfig, clock: XrayFeedSessionClock) -> Self {
        Self {
            recorder: Mutex::new(XrayReceiptRecorder::enabled(config)),
            clock,
        }
    }

    #[must_use]
    pub fn from_recorder(recorder: XrayReceiptRecorder, clock: XrayFeedSessionClock) -> Self {
        Self {
            recorder: Mutex::new(recorder),
            clock,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<XrayReceipt> {
        self.recorder
            .lock()
            .map(|recorder| recorder.snapshot())
            .unwrap_or_default()
    }
}

impl FeedSessionDiagnosticsSink for XrayFeedSessionRecorder {
    fn is_enabled(&self) -> bool {
        self.recorder
            .lock()
            .map(|recorder| recorder.is_enabled())
            .unwrap_or(false)
    }

    fn record(&self, batch: FeedSessionDiagnosticBatch) {
        let Ok(mut recorder) = self.recorder.lock() else {
            return;
        };
        if !recorder.is_enabled() {
            return;
        }
        let timestamp = (self.clock)();
        recorder.record_with(|| receipts_from_feed_session_batch(&batch, timestamp));
    }
}

#[must_use]
pub fn receipts_from_feed_session_batch(
    batch: &FeedSessionDiagnosticBatch,
    timestamp: XrayTimestamp,
) -> Vec<XrayReceipt> {
    let context = projection_context(batch);
    let transaction =
        XrayTransactionMarker::new(batch.transaction.transaction, batch.transaction.revision);
    batch
        .receipts
        .iter()
        .map(|receipt| receipt_from_feed_session(receipt, &context, transaction, timestamp))
        .collect()
}

fn receipt_from_feed_session(
    receipt: &FeedSessionDiagnosticReceipt,
    context: &XrayProjectionContext,
    transaction: XrayTransactionMarker,
    timestamp: XrayTimestamp,
) -> XrayReceipt {
    XrayReceipt::new(
        context.clone(),
        transaction,
        timestamp,
        event_kind(receipt.event),
        receipt.resource_id.clone(),
        receipt.interest.as_ref().map(interest_descriptor),
    )
    .with_owner_counts(XrayOwnerCounts::known(
        receipt.owner_counts.before,
        receipt.owner_counts.after,
    ))
}

fn projection_context(batch: &FeedSessionDiagnosticBatch) -> XrayProjectionContext {
    let context = XrayProjectionContext::new(
        batch.projection_key.clone(),
        batch.view_label.clone(),
        batch.owner_key.clone(),
        reason(&batch.reason),
    );
    if let Some(parent_scope) = &batch.parent_scope {
        context.with_parent_scope(parent_scope.clone())
    } else {
        context
    }
}

fn reason(reason: &FeedSessionDiagnosticReason) -> XrayReason {
    XrayReason::with_params(
        match reason.code {
            FeedSessionDiagnosticReasonCode::AcquisitionSync => XrayReasonCode::FeedSessionSync,
            FeedSessionDiagnosticReasonCode::SourceEffect => {
                XrayReasonCode::FeedSessionSourceEffect
            }
            FeedSessionDiagnosticReasonCode::AcquisitionClose => {
                XrayReasonCode::FeedSessionAcquisitionClose
            }
            FeedSessionDiagnosticReasonCode::Unknown => XrayReasonCode::Unknown,
        },
        vec![XrayReasonParam::new("label", reason.label.clone())],
    )
}

fn event_kind(event: FeedSessionDiagnosticEventKind) -> XrayReceiptEventKind {
    match event {
        FeedSessionDiagnosticEventKind::Open => XrayReceiptEventKind::Open,
        FeedSessionDiagnosticEventKind::Replace => XrayReceiptEventKind::Replace,
        FeedSessionDiagnosticEventKind::Refresh => XrayReceiptEventKind::Refresh,
        FeedSessionDiagnosticEventKind::Close => XrayReceiptEventKind::Close,
    }
}

fn interest_descriptor(interest: &FeedSessionDiagnosticInterest) -> XrayInterestDescriptor {
    XrayInterestDescriptor {
        interest_key: interest.interest_key.clone(),
        scope: interest.scope.clone(),
        shape: interest.shape.clone(),
        provenance: interest.provenance.clone(),
        privacy_bearing: interest.privacy_bearing,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicBool, Ordering};

    use nmp_feed_session::{
        FeedSessionDiagnosticOwnerCounts, FeedSessionDiagnosticReason,
        FeedSessionDiagnosticTransaction,
    };

    use super::*;

    fn batch() -> FeedSessionDiagnosticBatch {
        FeedSessionDiagnosticBatch {
            projection_key: "app.feed.home".to_string(),
            view_label: "root-indexed".to_string(),
            parent_scope: Some("scope:home".to_string()),
            owner_key: "sub-owner:7".to_string(),
            transaction: FeedSessionDiagnosticTransaction::new(11, 4),
            reason: FeedSessionDiagnosticReason::new(
                FeedSessionDiagnosticReasonCode::SourceEffect,
                "feed-session-acquisition",
            ),
            receipts: vec![
                FeedSessionDiagnosticReceipt {
                    event: FeedSessionDiagnosticEventKind::Open,
                    resource_id: "resource:a".to_string(),
                    interest: Some(FeedSessionDiagnosticInterest {
                        interest_key: "interest:a".to_string(),
                        scope: "active-account".to_string(),
                        shape: "lifecycle=tailing:shape=abc".to_string(),
                        provenance: "active-follow-timeline".to_string(),
                        privacy_bearing: true,
                    }),
                    owner_counts: FeedSessionDiagnosticOwnerCounts::known(0, 1),
                },
                FeedSessionDiagnosticReceipt {
                    event: FeedSessionDiagnosticEventKind::Close,
                    resource_id: "resource:a".to_string(),
                    interest: None,
                    owner_counts: FeedSessionDiagnosticOwnerCounts::known(1, 0),
                },
            ],
        }
    }

    #[test]
    fn feed_session_batches_convert_to_xray_receipts() {
        let receipts = receipts_from_feed_session_batch(&batch(), XrayTimestamp::new(99));

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].transaction, XrayTransactionMarker::new(11, 4));
        assert_eq!(receipts[0].timestamp, XrayTimestamp::new(99));
        assert_eq!(receipts[0].context.projection_key, "app.feed.home");
        assert_eq!(receipts[0].context.view_label, "root-indexed");
        assert_eq!(
            receipts[0].context.parent_scope.as_deref(),
            Some("scope:home")
        );
        assert_eq!(
            receipts[0].context.reason.code,
            XrayReasonCode::FeedSessionSourceEffect
        );
        assert_eq!(receipts[0].event, XrayReceiptEventKind::Open);
        assert_eq!(receipts[0].owner_counts, XrayOwnerCounts::known(0, 1));
        assert_eq!(
            receipts[0].interest.as_ref().unwrap().provenance,
            "active-follow-timeline"
        );
        assert_eq!(receipts[1].event, XrayReceiptEventKind::Close);
        assert!(receipts[1].interest.is_none());
    }

    #[test]
    fn live_sink_records_ordered_bounded_receipts() {
        let recorder = XrayFeedSessionRecorder::enabled(
            XrayRecordingConfig::new(NonZeroUsize::new(1).unwrap()),
            Arc::new(|| XrayTimestamp::new(7)),
        );

        recorder.record(batch());

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].sequence, 2);
        assert_eq!(snapshot[0].event, XrayReceiptEventKind::Close);
        assert_eq!(snapshot[0].timestamp, XrayTimestamp::new(7));
    }

    #[test]
    fn disabled_live_sink_does_not_call_clock_or_convert_batch() {
        let called = Arc::new(AtomicBool::new(false));
        let called_by_clock = Arc::clone(&called);
        let recorder = XrayFeedSessionRecorder::from_recorder(
            XrayReceiptRecorder::disabled(),
            Arc::new(move || {
                called_by_clock.store(true, Ordering::SeqCst);
                XrayTimestamp::new(7)
            }),
        );

        recorder.record(batch());

        assert!(!called.load(Ordering::SeqCst));
        assert!(recorder.snapshot().is_empty());
    }
}
