pub struct FeedSessions<'a> {
    app: &'a NmpApp,
}

pub struct FeedSessionHandle {
    pub projection_key: String,
    pub session_id: u64,
}

pub fn close_feed_session(app: &NmpApp, opened: &OpenedFeed) -> bool {
    opened
        .runtime_handle()
        .is_some_and(|handle| app.close_feed(&handle))
}
