//! Negative product_raw_read fixture.

pub fn product_shell_typed_reads(session: &mut FeedSession) {
    session.open_typed_feed(FeedParams {
        owner: "timeline".to_string(),
    });
    session.close_typed_feed("timeline");
    session.observe_typed_projection("timeline");
}

pub struct FeedSession;

impl FeedSession {
    pub fn open_typed_feed(&mut self, _params: FeedParams) {}

    pub fn close_typed_feed(&mut self, _owner: &str) {}

    pub fn observe_typed_projection(&mut self, _owner: &str) {}
}

pub struct FeedParams {
    pub owner: String,
}
