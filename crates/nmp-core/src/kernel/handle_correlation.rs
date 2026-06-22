//! `HandleCorrelationIndex` — durable publish-handle ↔ dispatch-`correlation_id`
//! index (S7, #1754).
//!
//! The publish engine keys its in-flight rows by the publish HANDLE (== signed
//! event id), but a dispatched action carries a registry-minted `correlation_id`
//! the host's spinner is bound to. The cancel-by-id doorway needs to resolve
//! between the two so the user-initiated `Cancelled` terminal lands under the
//! ORIGINAL `correlation_id` rather than the handle (the PD-036 fix). This index
//! is the bidirectional, bounded map that makes that resolution durable across
//! the engine's in-flight eviction.
//!
//! Lives beside `action_lifecycle` (it serves the lifecycle/cancel control
//! plane) but is split into its own file to keep each module under the 500-LOC
//! cap (AGENTS.md §file-size).

use std::collections::HashMap;

use super::action_stages::MAX_TRACKED_CORRELATIONS;

/// Cap on the durable handle↔correlation index. Mirrors
/// [`MAX_TRACKED_CORRELATIONS`]: a host that dispatches publishes faster than
/// they settle would otherwise grow the index unboundedly. Drop-oldest by
/// insertion order on overflow.
pub(crate) const MAX_HANDLE_CORRELATION_ENTRIES: usize = MAX_TRACKED_CORRELATIONS;

/// Durable bidirectional index between a publish HANDLE (== signed event id, the
/// key the publish engine tracks an in-flight publish under) and the ORIGINAL
/// dispatch `correlation_id` (the key the host's spinner is bound to).
///
/// # Why this exists (PD-036)
///
/// The publish engine keys its in-flight rows by the publish handle, but a
/// dispatched action carries a registry-minted `correlation_id` the host waits
/// on. Cancellation arrives addressing the `correlation_id` (that is the only id
/// the host knows). Without this index a cancel could only be matched against —
/// and its `Cancelled` terminal recorded under — the handle, which is NOT the
/// id the host's spinner is keyed on. This index lets the cancel-by-id doorway
/// resolve `correlation_id → handle` (to drive the engine's per-handle cancel)
/// while recording the terminal under the ORIGINAL `correlation_id`.
///
/// # Bidirectional, with self-mapping
///
/// The map is keyed by BOTH the correlation_id and the handle so the doorway can
/// accept either form and converge on the original correlation_id:
/// * `resolve(correlation_id) -> (handle, correlation_id)`
/// * `resolve(handle) -> (handle, correlation_id)`
///
/// For an internal publish that never received a distinct dispatch
/// correlation_id (the engine's fallback is "report the handle as the
/// correlation_id"), the handle maps to itself — so cancel-by-handle still works
/// and records the terminal under the handle (the only id the host has in that
/// case), preserving the prior behaviour for non-dispatch callers.
///
/// # Bounded (D8)
///
/// Insertion order is tracked; the oldest pair is evicted whole at
/// [`MAX_HANDLE_CORRELATION_ENTRIES`]. Settled publishes are removed via
/// [`Self::forget`], so the steady-state size tracks the engine's in-flight set.
#[derive(Default)]
pub(crate) struct HandleCorrelationIndex {
    /// `correlation_id` → publish `handle`. Also carries the handle→handle
    /// self-mapping so a raw-handle lookup resolves.
    correlation_to_handle: HashMap<String, String>,
    /// `handle` → original dispatch `correlation_id`. Also carries the
    /// correlation→correlation self-mapping for the same reason.
    handle_to_correlation: HashMap<String, String>,
    /// Insertion order of distinct handles, for drop-oldest eviction.
    handle_order: Vec<String>,
}

impl HandleCorrelationIndex {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record the `handle ↔ correlation_id` pairing for an in-flight publish.
    ///
    /// Called from the single engine-entry site that knows both ids. When
    /// `correlation_id` is `None` (an internal publish with no distinct dispatch
    /// id), the handle is mapped to itself so cancel-by-handle still resolves.
    /// Re-recording the same handle refreshes the mapping without duplicating
    /// the order entry.
    pub(crate) fn record(&mut self, handle: &str, correlation_id: Option<&str>) {
        let correlation_id = correlation_id.unwrap_or(handle);
        let is_new = !self.handle_to_correlation.contains_key(handle);
        if is_new && self.handle_order.len() >= MAX_HANDLE_CORRELATION_ENTRIES {
            if let Some(oldest) = self.handle_order.first().cloned() {
                self.forget(&oldest);
            }
        }
        self.handle_to_correlation
            .insert(handle.to_string(), correlation_id.to_string());
        // correlation_id → handle, plus a handle → handle self-mapping so a
        // lookup keyed on the raw handle also resolves.
        self.correlation_to_handle
            .insert(correlation_id.to_string(), handle.to_string());
        self.correlation_to_handle
            .insert(handle.to_string(), handle.to_string());
        // correlation → correlation self-mapping for the reverse direction.
        self.handle_to_correlation
            .insert(correlation_id.to_string(), correlation_id.to_string());
        if is_new {
            self.handle_order.push(handle.to_string());
        }
    }

    /// Resolve an id (either a `correlation_id` or a raw `handle`) to the
    /// `(handle, correlation_id)` pair. Returns `None` when the id is unknown —
    /// the caller falls back to treating the id as both (the prior
    /// cancel-by-handle behaviour for an already-evicted or never-indexed
    /// publish).
    #[must_use]
    pub(crate) fn resolve(&self, id: &str) -> Option<(String, String)> {
        let handle = self.correlation_to_handle.get(id)?;
        let correlation_id = self
            .handle_to_correlation
            .get(handle)
            .cloned()
            .unwrap_or_else(|| handle.clone());
        Some((handle.clone(), correlation_id))
    }

    /// Drop every mapping touching `handle` (and its correlation_id). Called when
    /// a publish settles so the index tracks the live in-flight set.
    pub(crate) fn forget(&mut self, handle: &str) {
        // Resolve the partnered correlation_id before removing, to clear both
        // directions and both self-mappings.
        let correlation_id = self.handle_to_correlation.get(handle).cloned();
        self.handle_to_correlation.remove(handle);
        self.correlation_to_handle.remove(handle);
        if let Some(cid) = correlation_id {
            self.correlation_to_handle.remove(&cid);
            self.handle_to_correlation.remove(&cid);
        }
        if let Some(pos) = self.handle_order.iter().position(|h| h == handle) {
            self.handle_order.remove(pos);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.handle_order.len()
    }
}

#[cfg(test)]
#[path = "handle_correlation_tests.rs"]
mod tests;
