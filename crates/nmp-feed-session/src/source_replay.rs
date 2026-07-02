use crate::FeedSessionHost;
use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_planner::InterestShape;
use std::num::NonZeroUsize;

pub(super) fn replay_source_shape(
    app: &impl FeedSessionHost,
    observer: &dyn ObservedProjectionSink,
    shape: InterestShape,
) {
    let pull = app.feed_pull_fn();
    replay_source_shape_with_pull(&pull, observer, shape);
}

pub(super) fn replay_source_shape_with_pull(
    pull: &nmp_feed::PullFn,
    observer: &dyn ObservedProjectionSink,
    shape: InterestShape,
) {
    let mut after_seq = 0;
    loop {
        let limits = nmp_core::PullLimits {
            max_entries: NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE)
                .unwrap_or(NonZeroUsize::MIN),
            max_scan_entries: NonZeroUsize::new(nmp_feed::DEFAULT_PULL_SCAN_BUDGET)
                .unwrap_or(NonZeroUsize::MIN),
        };
        match pull(
            nmp_core::PullScope::InterestShape(shape.clone()),
            after_seq,
            limits,
        ) {
            nmp_store::ScanLogResult::Page(page) => {
                for entry in page.entries {
                    let Some(raw) = entry.raw_event else {
                        continue;
                    };
                    observer.on_kernel_event(&KernelEvent {
                        id: raw.id,
                        author: raw.pubkey,
                        kind: raw.kind,
                        created_at: raw.created_at,
                        tags: raw.tags,
                        content: raw.content,
                        relay_provenance: Vec::new(),
                    });
                }
                if !page.has_more || page.next_after_seq == after_seq {
                    break;
                }
                after_seq = page.next_after_seq;
            }
            nmp_store::ScanLogResult::Gap(gap) => {
                if gap.first_available_seq == after_seq {
                    break;
                }
                after_seq = gap.first_available_seq;
            }
        }
    }
}
