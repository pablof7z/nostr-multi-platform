use std::collections::HashMap;

use crate::features::FeatureTab;
use crate::snapshot::FeedProjection;
use crate::timeline::TimelineRow;

use super::{AppRuntime, AppState, Pane};

pub(crate) trait DynamicFeedRuntime {
    fn open_author(&self, pubkey: &str) -> crate::Result<()>;
    fn close_author(&self, pubkey: &str) -> crate::Result<()>;
    fn open_thread(&self, event_id: &str) -> crate::Result<()>;
    fn close_thread(&self, event_id: &str) -> crate::Result<()>;
    /// ADR-0063 (#1671 Lane F): resolve the open profile pane's author at
    /// `profile.card` / `Live`. Best-effort — a failure must not abort the open.
    fn resolve_open_profile(&self, pubkey: &str) -> crate::Result<()>;
    /// Release the open profile pane's `profile.card` / `Live` ref on close.
    fn release_open_profile(&self, pubkey: &str) -> crate::Result<()>;
}

impl DynamicFeedRuntime for AppRuntime {
    fn open_author(&self, pubkey: &str) -> crate::Result<()> {
        AppRuntime::open_author(self, pubkey)
    }

    fn close_author(&self, pubkey: &str) -> crate::Result<()> {
        AppRuntime::close_author(self, pubkey)
    }

    fn resolve_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        AppRuntime::resolve_open_profile(self, pubkey)
    }

    fn release_open_profile(&self, pubkey: &str) -> crate::Result<()> {
        AppRuntime::release_open_profile(self, pubkey)
    }

    fn open_thread(&self, event_id: &str) -> crate::Result<()> {
        AppRuntime::open_thread(self, event_id)
    }

    fn close_thread(&self, event_id: &str) -> crate::Result<()> {
        AppRuntime::close_thread(self, event_id)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ClosedDynamicFeeds {
    pub author: Option<String>,
    pub thread: Option<String>,
}

impl ClosedDynamicFeeds {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.author.is_none() && self.thread.is_none()
    }

    #[must_use]
    pub(crate) fn status_label(&self) -> &'static str {
        match (self.author.is_some(), self.thread.is_some()) {
            (true, true) => "closed profile and thread feeds",
            (true, false) => "closed profile feed",
            (false, true) => "closed thread feed",
            (false, false) => "detail closed",
        }
    }
}

impl AppState {
    pub(crate) fn apply_dynamic_feeds(&mut self, feeds: &HashMap<String, FeedProjection>) {
        self.apply_author_feed(feeds);
        self.apply_thread_feed(feeds);
    }

    pub(crate) fn open_author_feed(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
        pubkey: &str,
    ) -> crate::Result<()> {
        self.close_dynamic_feeds(runtime)?;
        runtime.open_author(pubkey)?;
        // ADR-0063 (#1671 Lane F): the open profile pane wants the full card,
        // kept Live for reactive kind:0 replacement. Best-effort (the pane still
        // opens if the resolve errors; an invalid pubkey would have failed
        // open_author first).
        let _ = runtime.resolve_open_profile(pubkey);
        self.profile_pubkey = pubkey.to_string();
        self.profile_rows.clear();
        self.focus(Pane::Profile);
        Ok(())
    }

    pub(crate) fn open_thread_feed(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
        event_id: &str,
    ) -> crate::Result<()> {
        self.close_dynamic_feeds(runtime)?;
        runtime.open_thread(event_id)?;
        self.thread_event_id = event_id.to_string();
        self.thread_rows.clear();
        self.detail_cursor = 0;
        self.detail_scroll = 0;
        self.focus(Pane::Detail);
        Ok(())
    }

    pub(crate) fn close_author_feed(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
    ) -> crate::Result<Option<String>> {
        if self.profile_pubkey.is_empty() {
            return Ok(None);
        }
        let pubkey = self.profile_pubkey.clone();
        runtime.close_author(&pubkey)?;
        // ADR-0063 (#1671 Lane F): release the open-pane profile.card/Live ref so
        // the slot drops back to whatever feed rows still demand (D5 bounded).
        let _ = runtime.release_open_profile(&pubkey);
        self.profile_pubkey.clear();
        self.profile_rows.clear();
        Ok(Some(pubkey))
    }

    pub(crate) fn close_thread_feed(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
    ) -> crate::Result<Option<String>> {
        if self.thread_event_id.is_empty() {
            return Ok(None);
        }
        let event_id = self.thread_event_id.clone();
        runtime.close_thread(&event_id)?;
        self.thread_event_id.clear();
        self.thread_rows.clear();
        self.detail_cursor = 0;
        self.detail_scroll = 0;
        Ok(Some(event_id))
    }

    pub(crate) fn close_dynamic_feeds(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
    ) -> crate::Result<ClosedDynamicFeeds> {
        let author = self.close_author_feed(runtime)?;
        let thread = self.close_thread_feed(runtime)?;
        Ok(ClosedDynamicFeeds { author, thread })
    }

    pub(crate) fn close_current_dynamic_view(
        &mut self,
        runtime: &impl DynamicFeedRuntime,
    ) -> crate::Result<ClosedDynamicFeeds> {
        let closed = match self.focused {
            Pane::Profile => ClosedDynamicFeeds {
                author: self.close_author_feed(runtime)?,
                thread: None,
            },
            Pane::Detail => ClosedDynamicFeeds {
                author: None,
                thread: self.close_thread_feed(runtime)?,
            },
            Pane::Feed => self.close_dynamic_feeds(runtime)?,
        };
        if !closed.is_empty() {
            self.focus(Pane::Feed);
        }
        Ok(closed)
    }

    #[must_use]
    pub fn render_intent_rows(&self) -> &[TimelineRow] {
        if self.tab != FeatureTab::Home {
            return &[];
        }
        match self.focused {
            Pane::Profile if !self.profile_pubkey.is_empty() => &self.profile_rows,
            Pane::Detail if !self.thread_event_id.is_empty() => &self.thread_rows,
            _ => &self.rows,
        }
    }

    fn apply_author_feed(&mut self, feeds: &HashMap<String, FeedProjection>) {
        if self.profile_pubkey.is_empty() {
            return;
        }
        let key = author_feed_key(&self.profile_pubkey);
        match feeds.get(&key) {
            Some(FeedProjection::Changed(feed)) => {
                self.profile_rows = TimelineRow::from_snapshot(feed);
            }
            Some(FeedProjection::Cleared) => {
                self.profile_pubkey.clear();
                self.profile_rows.clear();
                if self.focused == Pane::Profile {
                    self.focus(Pane::Feed);
                }
            }
            None => {}
        }
    }

    fn apply_thread_feed(&mut self, feeds: &HashMap<String, FeedProjection>) {
        if self.thread_event_id.is_empty() {
            return;
        }
        let key = thread_feed_key(&self.thread_event_id);
        match feeds.get(&key) {
            Some(FeedProjection::Changed(feed)) => {
                self.thread_rows = TimelineRow::from_snapshot(feed);
                if self.detail_cursor >= self.thread_rows.len() {
                    self.detail_cursor = self.thread_rows.len().saturating_sub(1);
                }
            }
            Some(FeedProjection::Cleared) => {
                self.thread_event_id.clear();
                self.thread_rows.clear();
                self.detail_cursor = 0;
                self.detail_scroll = 0;
                if self.focused == Pane::Detail {
                    self.focus(Pane::Feed);
                }
            }
            None => {}
        }
    }
}

fn author_feed_key(pubkey: &str) -> String {
    format!("nmp.feed.author.{pubkey}")
}

fn thread_feed_key(event_id: &str) -> String {
    format!("nmp.feed.thread.{event_id}")
}
