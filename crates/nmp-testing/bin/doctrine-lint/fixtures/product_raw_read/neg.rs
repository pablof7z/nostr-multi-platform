//! Negative product_raw_read fixture.

pub fn product_shell_typed_reads(session: &mut FeedSession) {
    session.open_typed_feed(FeedParams {
        owner: "timeline".to_string(),
    });
}

pub struct FeedSession;

impl FeedSession {
    pub fn open_typed_feed(&mut self, _params: FeedParams) {}
}

pub struct FeedParams {
    pub owner: String,
}
