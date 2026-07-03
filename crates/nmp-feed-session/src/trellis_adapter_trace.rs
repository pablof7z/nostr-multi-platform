use trellis_core::{OutputFrame, OutputFrameKind, ResourceCommand};

use crate::trellis_resources::FeedSessionResourceCommand;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FeedSessionOutputFrameKind {
    Baseline,
    Delta,
    Rebaseline,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FeedSessionResourceTraceKind {
    Open,
    Replace,
    Refresh,
    Close,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct FeedSessionResourceTrace {
    pub(super) kind: FeedSessionResourceTraceKind,
    pub(super) key: String,
}

pub(super) fn output_frame_kinds(frames: &[OutputFrame]) -> Vec<FeedSessionOutputFrameKind> {
    frames
        .iter()
        .map(|frame| match frame.kind {
            OutputFrameKind::Baseline(_) => FeedSessionOutputFrameKind::Baseline,
            OutputFrameKind::Delta(_) => FeedSessionOutputFrameKind::Delta,
            OutputFrameKind::Rebaseline(_, _) => FeedSessionOutputFrameKind::Rebaseline,
            OutputFrameKind::Clear(_) => FeedSessionOutputFrameKind::Clear,
        })
        .collect()
}

pub(super) fn resource_traces(
    commands: &[ResourceCommand<FeedSessionResourceCommand>],
) -> Vec<FeedSessionResourceTrace> {
    commands
        .iter()
        .map(|command| {
            let kind = match command {
                ResourceCommand::Open { .. } => FeedSessionResourceTraceKind::Open,
                ResourceCommand::Replace { .. } => FeedSessionResourceTraceKind::Replace,
                ResourceCommand::Refresh { .. } => FeedSessionResourceTraceKind::Refresh,
                ResourceCommand::Close { .. } => FeedSessionResourceTraceKind::Close,
            };
            FeedSessionResourceTrace {
                kind,
                key: command.key().as_str().to_string(),
            }
        })
        .collect()
}
