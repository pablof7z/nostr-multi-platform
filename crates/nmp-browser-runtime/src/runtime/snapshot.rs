//! Fail-closed snapshot decode/merge cache for the browser runtime (#2073).
//!
//! `BrowserSnapshotCache` wraps [`ProjectionMergeCache`] with a `last_good`
//! buffer. Every call to [`BrowserSnapshotCache::apply_frame`] either:
//! - Returns `SnapshotOutcome::Frame(merged)` and updates `last_good`, or
//! - Returns `SnapshotOutcome::Degraded` (transient decode error — `last_good`
//!   is NOT overwritten; the host still has a valid prior frame), or
//! - Returns `SnapshotOutcome::Panic(msg)` (terminal — the kernel panicked;
//!   the runtime should surface the panic and stop further pumping).
//!
//! The policy is **never ship an undecodable frame**: on any transient error,
//! the host receives the previous good frame (or `None` if no good frame yet).
//! A panic frame is always terminal (the kernel is in an irrecoverable state).
//!
//! D6 — total: every path is `Result`-gated; `last_good` is never mutated
//! on error; the cache never panics.

use nmp_core::{ProjectionMergeCache, UpdateFrameDecodeError};

/// The outcome of one [`BrowserSnapshotCache::apply_frame`] call.
#[derive(Debug, Clone)]
pub enum SnapshotOutcome {
    /// A valid merged frame — ship it to the host.
    Frame(Vec<u8>),
    /// Transient decode/merge error. The host receives the last good frame
    /// (if any). A [`super::event::BrowserRuntimeEvent::SnapshotDecodeFailed`]
    /// event is also emitted so the error is observable (D6 — no silent drop).
    Degraded {
        /// The last successfully merged frame, or `None` on the very first frame.
        last_good: Option<Vec<u8>>,
        /// Human-readable error reason (category, not internal body text).
        reason: String,
    },
    /// Terminal: the kernel emitted a panic frame. The runtime should surface
    /// the message and stop producing further frames (the kernel is dead).
    Panic(String),
}

/// Stateful merge cache that owns the `last_good` snapshot buffer.
///
/// Constructed once at `start()` and held by `BrowserRuntimeHandle`. The
/// `apply_frame` method applies one raw `make_update_frame` output.
#[derive(Default)]
pub struct BrowserSnapshotCache {
    merge_cache: ProjectionMergeCache,
    last_good: Option<Vec<u8>>,
}

impl BrowserSnapshotCache {
    /// Construct a fresh cache with no prior good frame.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply `raw_frame_bytes` (as returned by `KernelReducer::make_update_frame`)
    /// and return a [`SnapshotOutcome`].
    ///
    /// On success: stores `merged` as `last_good` and returns `Frame(merged)`.
    /// On `UnexpectedPanicFrame`: does NOT touch `last_good`; returns `Panic(msg)`.
    /// On any other error: does NOT touch `last_good`; returns `Degraded`.
    ///
    /// D6 — total: never panics; all error paths are `Result`-gated.
    pub fn apply_frame(&mut self, raw: &[u8]) -> SnapshotOutcome {
        match self.merge_cache.merge_update_frame(raw) {
            Ok(merged) => {
                self.last_good = Some(merged.clone());
                SnapshotOutcome::Frame(merged)
            }
            Err(UpdateFrameDecodeError::UnexpectedPanicFrame(msg)) => {
                // Terminal — the kernel panicked. Do NOT update last_good.
                SnapshotOutcome::Panic(msg)
            }
            Err(other) => {
                // Transient decode/merge error. last_good is preserved.
                let reason = other.to_string();
                SnapshotOutcome::Degraded {
                    last_good: self.last_good.clone(),
                    reason,
                }
            }
        }
    }

    /// The last successfully merged frame, or `None` if no frame has decoded yet.
    #[must_use]
    pub fn last_good(&self) -> Option<&[u8]> {
        self.last_good.as_deref()
    }
}
