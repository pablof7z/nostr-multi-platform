pub struct Feeds<'a> {
    app: &'a NmpApp,
}

pub struct FeedHandle {
    pub projection_key: String,
    pub handle_id: u64,
}

pub fn close_feed(app: &NmpApp, opened: &OpenedFeed) -> bool {
    opened
        .runtime_handle()
        .is_some_and(|handle| app.close_feed(&handle))
}

// Untouched internal-runtime and other-domain "session" vocabulary must stay
// clean under this ratchet — it is a different, still-legitimate surface.
pub struct FeedSessionRegistry;
pub struct FeedSessionId(pub u64);
pub struct Nip50SearchSession;
pub struct ActiveFollowsOpFeedSession;
