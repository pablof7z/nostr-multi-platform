//! GENERATED (derisk proof for #2899) — `open_replies` / `close_replies`
//! concept-read slice.
//!
//! This file is hand-authored in the EXACT shape the future `nmp gen
//! concept-reads` emitter (#2899 Part B, out of scope for this PR) would
//! produce for one door: a `#[uniffi::export] impl GalleryApp` block in its
//! own module, calling straight through to the concept crate's bridge-lane
//! primitives (`nmp_replies::decode_and_validate_reply_target`,
//! `open_replies`/`close_replies`, `RepliesReadHandle::into_parts`/
//! `from_parts`) added in Part A. It contains NO policy of its own — no
//! handle-map, no retry logic, no protocol decisions — only the mechanical
//! marshal a generator would emit.
//!
//! # What this proves (#2899 DERISK)
//!
//! The one flagged unknown was whether a CHECKED-IN GENERATED
//! `#[uniffi::export] impl` block, living in its own file/module rather than
//! alongside the type's `#[derive(uniffi::Object)]`, resolves through
//! UniFFI's proc-macro crate-name detection identically to a hand-written
//! block in `facade.rs`. It does: UniFFI's export macros key every emitted
//! symbol off `CARGO_PKG_NAME`/the crate's own metadata at expansion time,
//! not off which source file the macro invocation appears in — so multiple
//! `#[uniffi::export] impl GalleryApp` blocks across separate files in the
//! SAME crate compile and link into the SAME facade namespace. See the
//! in-crate tests below (and `cargo build`/`cargo test -p nmp-app-gallery`
//! succeeding at all) for the compiled proof; a real UniFFI namespace
//! collision or unresolved-crate failure would be a hard compile error, not a
//! silent behavior difference.
//!
//! # No handle map (acceptance criteria)
//!
//! `open_replies` returns `(projection_key, handle_id)` as a flat
//! [`GalleryOpenedReplies`] record; `close_replies` takes that SAME record
//! back and reconstructs the typed `RepliesReadHandle` via
//! [`nmp_replies::RepliesReadHandle::from_parts`] to close. There is no
//! facade-owned `Mutex<HashMap<_, RepliesReadHandle>>` anywhere in this file
//! — the round trip through scalar parts (Part A) is what makes that
//! unnecessary.

use nmp_replies::{
    close_replies, decode_and_validate_reply_target, open_replies, RepliesReadHandle,
};

use crate::facade::GalleryApp;

/// The opaque (projection_key, handle_id) pair `open_replies` returns and
/// `close_replies` consumes to close — the FFI-marshalable form of
/// [`nmp_replies::RepliesReadHandle`] (#2899 Part A `into_parts`/
/// `from_parts`).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct GalleryOpenedReplies {
    pub projection_key: String,
    pub handle_id: u64,
}

/// Why `open_replies` failed. Facade-local error namespace mapping the
/// concept crate's stable `code()` strings onto a small closed set a shell
/// switches on (mirrors the `code()` values Part A added to
/// `nmp_replies::ReplyTargetError`/`ReplyTargetParamsError`/
/// `ReplyReadPlanError`, without re-exporting those Rust-side enums across
/// the FFI boundary).
#[derive(uniffi::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryReadError {
    /// `target_json` was malformed, or the decoded target failed validation
    /// (e.g. a non-hex event id, or a kind:1111 target supplied via the bare
    /// `event` shape instead of `comment`).
    InvalidTarget,
    /// The target decoded, but the read-plan compiler rejected it (e.g. a
    /// kind:1111 event target whose `CommentRecord` failed to decode from its
    /// own tags).
    OpenFailed,
}

impl core::fmt::Display for GalleryReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidTarget => write!(f, "invalid reply target"),
            Self::OpenFailed => write!(f, "open_replies rejected the read plan"),
        }
    }
}

impl std::error::Error for GalleryReadError {}

#[uniffi::export]
impl GalleryApp {
    /// Open a reply-count read for the target described by `target_json`
    /// (the `nmp_replies::ReplyTargetParams` JSON shape — Part A's
    /// FFI-marshalable input), returning the opaque scalar handle a shell
    /// carries until it calls [`Self::close_replies`].
    pub fn open_replies(
        &self,
        target_json: String,
    ) -> Result<GalleryOpenedReplies, GalleryReadError> {
        let target = decode_and_validate_reply_target(&target_json)
            .map_err(|_| GalleryReadError::InvalidTarget)?;
        let handle =
            open_replies(self.runtime(), target).map_err(|_| GalleryReadError::OpenFailed)?;
        let (projection_key, handle_id) = handle.into_parts();
        Ok(GalleryOpenedReplies {
            projection_key,
            handle_id,
        })
    }

    /// Close a reply read opened by [`Self::open_replies`]. Idempotent (D6):
    /// closing an already-closed or unknown handle is a safe no-op.
    pub fn close_replies(&self, opened: GalleryOpenedReplies) -> bool {
        let handle = RepliesReadHandle::from_parts(opened.projection_key, opened.handle_id);
        close_replies(self.runtime(), handle)
    }
}

#[cfg(test)]
#[path = "concept_reads_replies_tests.rs"]
mod tests;
