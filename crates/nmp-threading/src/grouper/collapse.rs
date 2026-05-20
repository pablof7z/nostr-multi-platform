use nmp_core::substrate::KernelEvent;

use crate::block::TimelineBlock;
use crate::pointer::ThreadPointer;
use crate::resolver::ParentResolver;

use super::Grouper;

impl<R: ParentResolver> Grouper<R> {
    /// Merge adjacent `Module` blocks sharing the same `root` pointer, when
    /// policy permits and combined length fits `max_module_size`.
    pub(super) fn collapse_adjacent(&mut self) {
        if !self.policy.collapse_adjacent_same_root {
            return;
        }
        let max_size = self.policy.max_module_size as usize;
        let mut i = 0;
        while i + 1 < self.blocks.len() {
            let merge = match (&self.blocks[i], &self.blocks[i + 1]) {
                (
                    TimelineBlock::Module {
                        events: e_a,
                        root: Some(r_a),
                        ..
                    },
                    TimelineBlock::Module {
                        events: e_b,
                        root: Some(r_b),
                        ..
                    },
                ) if r_a == r_b => e_a.len() + e_b.len() <= max_size,
                _ => false,
            };
            if merge {
                // Block i is newer, i+1 is older. Merged chain order is
                // older.events ++ newer.events (root-first preserved).
                let TimelineBlock::Module {
                    events: newer_events,
                    has_gap: newer_gap,
                    root,
                } = self.blocks.remove(i)
                else {
                    unreachable!() // doctrine-allow: D6 — let-else: merge is only set when blocks[i] is Module
                };
                let TimelineBlock::Module {
                    events: mut older_events,
                    has_gap: older_gap,
                    ..
                } = self.blocks.remove(i)
                else {
                    unreachable!() // doctrine-allow: D6 — let-else: merge is only set when blocks[i+1] is Module
                };
                older_events.extend(newer_events);
                self.blocks.insert(
                    i,
                    TimelineBlock::Module {
                        events: older_events,
                        has_gap: newer_gap || older_gap,
                        root,
                    },
                );
                // Don't advance; the merged block may collapse further.
            } else {
                i += 1;
            }
        }
    }
}

pub(super) fn gap_between(
    parent: Option<&KernelEvent>,
    child: Option<&KernelEvent>,
    threshold_secs: u64,
) -> bool {
    match (parent, child) {
        (Some(p), Some(c)) => c.created_at.saturating_sub(p.created_at) > threshold_secs,
        _ => false,
    }
}

/// True when the chain's root pointer names an Event id different from the
/// chain's top element (i.e. the module does not contain its declared root).
/// Address / External / None always returns false — non-Event roots are
/// handled by the terminal-walk branch instead.
pub(super) fn root_id_mismatched(root: Option<&ThreadPointer>, chain_top: &str) -> bool {
    match root {
        Some(ThreadPointer::Event { id, .. }) => id != chain_top,
        _ => false,
    }
}
