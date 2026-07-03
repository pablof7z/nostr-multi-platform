use nmp_core::subs::SubOwnerKey;
use nmp_feed::{FeedShape, ProjectionKey};
use trellis_core::TransactionResult;

use crate::diagnostics::{
    FeedSessionDiagnosticBatch, FeedSessionDiagnosticContext, FeedSessionDiagnosticReason,
    FeedSessionDiagnosticReasonCode, FeedSessionDiagnosticReceipt,
    FeedSessionDiagnosticTransaction,
};
use crate::trellis_resources::{shape_part, FeedSessionResourceCommand, FeedSessionScopeKey};

pub(super) fn diagnostic_context(
    projection: &ProjectionKey,
    shape: &FeedShape,
    scope_key: &FeedSessionScopeKey,
    owner: SubOwnerKey,
) -> FeedSessionDiagnosticContext {
    FeedSessionDiagnosticContext::new(
        projection.as_str(),
        shape_part(shape),
        Some(scope_key.as_str().to_string()),
        format!("sub-owner:{}", owner.0),
    )
}

pub(super) fn diagnostic_batch(
    context: &FeedSessionDiagnosticContext,
    receipts: Vec<FeedSessionDiagnosticReceipt>,
    result: &TransactionResult<FeedSessionResourceCommand>,
    reason: FeedSessionDiagnosticReasonCode,
    reason_label: &'static str,
) -> Option<FeedSessionDiagnosticBatch> {
    if receipts.is_empty() {
        return None;
    }
    Some(FeedSessionDiagnosticBatch {
        projection_key: context.projection_key.clone(),
        view_label: context.view_label.clone(),
        parent_scope: context.parent_scope.clone(),
        owner_key: context.owner_key.clone(),
        transaction: FeedSessionDiagnosticTransaction::new(
            result.transaction_id.get(),
            result.revision.get(),
        ),
        reason: FeedSessionDiagnosticReason::new(reason, reason_label),
        receipts,
    })
}
