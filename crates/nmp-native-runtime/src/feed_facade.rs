//! App-facing feed-session facade.
//!
//! `NmpApp::open_feed` is already the canonical implementation seam: it hides
//! compiler selection, validates `FeedParams`, opens the session, and returns a
//! handle-owned lifecycle token. This facade gives app Rust code the north-star
//! `app.feeds().open(...)` shape without introducing a second feed engine.

use crate::{FeedHandle, FeedOpenError, FeedParams, NmpApp};

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

    /// Page an open feed by its returned handle.
    #[must_use]
    pub fn load_older(&self, handle: &FeedHandle) -> bool {
        self.app.load_older_feed(handle)
    }

    /// Close an open feed by its returned handle.
    #[must_use]
    pub fn close(&self, handle: &FeedHandle) -> bool {
        self.app.close_feed(handle)
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
