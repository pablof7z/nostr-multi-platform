use std::fmt;

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};

use crate::diagnostics::FeedSessionDiagnosticReasonCode;
use crate::source::ExtraAcquisition;
use crate::trellis_adapter::FeedSessionTrellisAdapter;

pub(super) struct FeedSessionTrellisCommand {
    adapter: FeedSessionTrellisAdapter,
    operation: FeedSessionTrellisOperation,
}

enum FeedSessionTrellisOperation {
    SourceEffect {
        extra: ExtraAcquisition,
        reason: &'static str,
        rebaseline: bool,
    },
}

impl FeedSessionTrellisCommand {
    pub(super) fn source_effect(
        adapter: FeedSessionTrellisAdapter,
        extra: ExtraAcquisition,
        reason: &'static str,
        rebaseline: bool,
    ) -> Self {
        Self {
            adapter,
            operation: FeedSessionTrellisOperation::SourceEffect {
                extra,
                reason,
                rebaseline,
            },
        }
    }
}

impl fmt::Debug for FeedSessionTrellisCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedSessionTrellisCommand")
            .field("operation", &self.operation.label())
            .finish()
    }
}

impl FeedSessionTrellisOperation {
    fn label(&self) -> &'static str {
        match self {
            FeedSessionTrellisOperation::SourceEffect { .. } => "source-effect",
        }
    }
}

impl ProtocolCommand for FeedSessionTrellisCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        match self.operation {
            FeedSessionTrellisOperation::SourceEffect {
                extra,
                reason,
                rebaseline,
            } => {
                self.adapter.sync_with_diagnostic_reason(
                    &extra,
                    reason,
                    FeedSessionDiagnosticReasonCode::SourceEffect,
                );
                self.adapter.rebaseline_output_if_changed(rebaseline);
            }
        }
        Ok(())
    }
}
