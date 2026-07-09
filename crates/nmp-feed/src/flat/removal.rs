//! Row/source removal mechanics for [`super::FlatFeed`], split out of
//! `flat.rs` to keep it under the file-size gate (AGENTS.md).
//!
//! A child module of `flat` — Rust's default privacy (visible to the
//! defining module and its descendants) lets these methods reach
//! `FlatFeed`'s private fields (`state`/`merge`/`source_removed`) and
//! `FlatRow`'s private fields exactly as if they were still declared in
//! `flat.rs` itself.

use super::{merge_sources, FlatFeed, FlatFeedItem};
use serde::Serialize;

impl<C> FlatFeed<C>
where
    C: Clone + Send + Serialize + 'static,
{
    /// Remove an entire canonical row by id.
    ///
    /// Protocol adapters use this for deletes, mutes, blocks, and other
    /// externally-owned suppression facts that apply to the target event. The
    /// generic feed does not interpret those policies; it only owns row-index
    /// mutation. Fires [`Self::source_removed`]'s hook once per source
    /// contribution the row held (#3087) — a row can hold several sources
    /// sharing one canonical id, and each is an independent declarer as far
    /// as any per-source demand (e.g. `DeliveredRefDemand`) is concerned.
    pub fn remove_item(&self, id: &str) -> bool {
        let removed_sources: Option<Vec<String>> = self
            .state
            .lock()
            .ok()
            .and_then(|mut st| st.rows.remove(id))
            .map(|row| row.sources.into_keys().collect());
        let removed = removed_sources.is_some();
        for source_id in removed_sources.into_iter().flatten() {
            self.notify_source_removed(&source_id);
        }
        removed
    }

    /// Remove an entire canonical row when the current best card satisfies
    /// `predicate`.
    pub fn remove_item_if(&self, id: &str, predicate: impl FnOnce(&C) -> bool) -> bool {
        let removed_sources: Option<Vec<String>> = {
            let Ok(mut st) = self.state.lock() else {
                return false;
            };
            let should_remove = st.rows.get(id).is_some_and(|row| predicate(&row.best.card));
            if should_remove {
                st.rows.remove(id).map(|row| row.sources.into_keys().collect())
            } else {
                None
            }
        };
        let removed = removed_sources.is_some();
        for source_id in removed_sources.into_iter().flatten() {
            self.notify_source_removed(&source_id);
        }
        removed
    }

    /// Remove one source contribution from a canonical row and recompute the
    /// row from remaining sources. Fires [`Self::source_removed`]'s hook for
    /// `source_id` whenever it was actually present, regardless of whether
    /// the row survives with other sources or is itself emptied out (#3087) —
    /// that ONE source's own declared demand must retract either way.
    pub fn remove_source(&self, id: &str, source_id: &str) -> bool {
        let removed = {
            let Ok(mut st) = self.state.lock() else {
                return false;
            };
            let Some(row) = st.rows.get_mut(id) else {
                return false;
            };
            if row.sources.remove(source_id).is_none() {
                return false;
            }
            if let Some(best) = merge_sources(&self.merge, &row.sources) {
                row.best = best;
            } else {
                st.rows.remove(id);
            }
            true
        };
        if removed {
            self.notify_source_removed(source_id);
        }
        removed
    }

    /// Remove all source contributions matching `predicate`.
    ///
    /// Returns the number of removed sources. Canonical rows with remaining
    /// sources are recomputed; rows with no sources left are removed. Fires
    /// [`Self::source_removed`]'s hook once per matched source id (#3087),
    /// regardless of whether its row survives or empties out.
    pub fn remove_sources_if(&self, predicate: impl Fn(&FlatFeedItem<C>) -> bool) -> usize {
        let removed_source_ids: Vec<String> = {
            let Ok(mut st) = self.state.lock() else {
                return 0;
            };

            let mut removed_source_ids = Vec::new();
            let mut empty_rows = Vec::new();
            for (id, row) in &mut st.rows {
                let matched: Vec<String> = row
                    .sources
                    .iter()
                    .filter(|(_, item)| predicate(item))
                    .map(|(source_id, _)| source_id.clone())
                    .collect();
                for source_id in &matched {
                    row.sources.remove(source_id);
                }
                removed_source_ids.extend(matched);
                if let Some(best) = merge_sources(&self.merge, &row.sources) {
                    row.best = best;
                } else {
                    empty_rows.push(id.clone());
                }
            }
            for id in &empty_rows {
                st.rows.remove(id);
            }
            removed_source_ids
        };
        for source_id in &removed_source_ids {
            self.notify_source_removed(source_id);
        }
        removed_source_ids.len()
    }

    /// Invoke the source-removal hook (if any) for a source contribution that
    /// was just dropped from `state.rows`. Called with the state lock
    /// released so a hook may safely re-enter the feed (e.g. re-querying
    /// `len()`).
    fn notify_source_removed(&self, source_id: &str) {
        if let Some(hook) = &self.source_removed {
            hook(source_id);
        }
    }
}
