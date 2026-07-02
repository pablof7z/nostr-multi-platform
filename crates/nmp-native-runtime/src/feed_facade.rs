//! App-facing feed-session facade.
//!
//! `NmpApp::open_feed` is already the canonical implementation seam: it hides
//! compiler selection, validates `FeedParams`, opens the session, and returns a
//! handle-owned lifecycle token. This facade gives app Rust code the north-star
//! `app.feeds().open_spec(feed_key, feed_spec)` shape without introducing a
//! second feed engine.

use std::fmt;

use crate::{
    FeedHandle, FeedKey, FeedLoadStatus, FeedOpenError, FeedParams, FeedSpec, FeedSpecError, NmpApp,
};

/// Borrowed app-facing feed-session API.
///
/// This type owns no state. Every method delegates to the existing
/// handle-owned `NmpApp` feed lifecycle, so close/pagination continue to be
/// authorized by the returned [`FeedHandle`], not by replaying keys or params.
pub struct FeedSessions<'a> {
    app: &'a NmpApp,
}

impl<'a> FeedSessions<'a> {
    pub(crate) fn new(app: &'a NmpApp) -> Self {
        Self { app }
    }

    /// Open a feed through the standard NMP feed compiler.
    ///
    /// Callers provide typed [`FeedParams`] only; compiler selection, observer
    /// registration, source hooks, pull controllers, and teardown recipes remain
    /// internal runtime machinery.
    pub fn open(&self, params: &FeedParams) -> Result<FeedHandle, FeedOpenError> {
        self.app.open_feed(params)
    }

    /// Open an ergonomic feed spec through the standard NMP feed compiler.
    ///
    /// The spec is first compiled into canonical [`FeedParams`]. The session
    /// then follows the same compiler, registry, projection, pagination, and
    /// teardown path as [`Self::open`].
    pub fn open_spec(&self, key: FeedKey, spec: FeedSpec) -> Result<FeedHandle, FeedSpecOpenError> {
        let params = spec
            .into_params(key)
            .map_err(FeedSpecOpenError::InvalidSpec)?;
        self.open(&params).map_err(FeedSpecOpenError::OpenFailed)
    }

    /// Page an open feed by its returned handle.
    #[must_use]
    pub fn load_older(&self, handle: &FeedHandle) -> bool {
        self.app.load_older_feed(handle)
    }

    /// Page an open feed and return the Rust-owned stop reason.
    #[must_use]
    pub fn load_older_status(&self, handle: &FeedHandle) -> FeedLoadStatus {
        self.app.load_older_feed_status(handle)
    }

    /// Close an open feed by its returned handle.
    #[must_use]
    pub fn close(&self, handle: &FeedHandle) -> bool {
        self.app.close_feed(handle)
    }
}

/// Typed failure for [`FeedSessions::open_spec`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedSpecOpenError {
    /// The ergonomic spec did not contain enough app intent to build params.
    InvalidSpec(FeedSpecError),
    /// The canonical feed compiler rejected the built params or runtime wiring.
    OpenFailed(FeedOpenError),
}

impl fmt::Display for FeedSpecOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedSpecOpenError::InvalidSpec(err) => write!(f, "{err}"),
            FeedSpecOpenError::OpenFailed(err) => write!(f, "{err:?}"),
        }
    }
}

impl NmpApp {
    /// App-facing feed-session facade.
    ///
    /// This is the normal Rust app doorway for feed-shaped typed sessions:
    /// open typed params, page by returned handle, close by returned handle.
    #[must_use]
    pub fn feeds(&self) -> FeedSessions<'_> {
        FeedSessions::new(self)
    }
}

#[cfg(test)]
#[path = "feed_facade_tests.rs"]
mod tests;
