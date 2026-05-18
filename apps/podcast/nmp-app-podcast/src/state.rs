//! `PodcastApp` — the per-app projection state for the podcast library.
//!
//! Owns a `Mutex<Inner>` carrying the list of subscribed podcasts. The FFI
//! layer is the only caller; Swift drives every mutation/snapshot from its
//! own dispatch serialization, but `Mutex` gives us a clean "single shared
//! mutable state" boundary regardless of the platform thread model.
//!
//! State is intentionally in-memory only for this iteration — persistence
//! lands when the kernel's domain-store wiring is exercised (the design doc
//! `docs/design/podcast/podcast-core.md` §B promises LMDB-backed records).
//! Filed as a follow-up task.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use podcast_core::domain::ids::PodcastId;
use podcast_core::domain::records::PodcastRecord;
use podcast_core::views::{LibraryView, PodcastRowPayload};
use ulid::Ulid;
use url::Url;

/// Output of [`PodcastApp::subscribe`]. Mirrors
/// `podcast_core::actions::SubscribePodcastOutput` but kept local to the
/// app-state crate because the FFI surface decides the JSON shape; nothing
/// crosses the boundary yet that names the typed action enum.
#[derive(Debug, Clone, PartialEq)]
pub enum SubscribeResult {
    Subscribed { podcast_id: PodcastId },
    AlreadySubscribed { podcast_id: PodcastId },
}

/// Per-app state. Holds the in-memory library; future iterations replace
/// the `Vec` with a kernel-domain-store-backed read view.
#[derive(Default)]
pub struct PodcastApp {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    podcasts: Vec<PodcastRecord>,
}

impl PodcastApp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to a feed. Dedupes on `feed_url` — repeated calls return
    /// the existing `podcast_id`. Title/author are optional metadata; when
    /// missing we fall back to the host portion of the URL so the UI can
    /// still render something stable.
    pub fn subscribe(
        &self,
        feed_url: Url,
        title: Option<String>,
        author: Option<String>,
    ) -> SubscribeResult {
        let Ok(mut inner) = self.inner.lock() else {
            // D6 — poisoned mutex returns a fresh-id Subscribed; the next
            // successful call will reconcile. We never panic across FFI.
            return SubscribeResult::Subscribed {
                podcast_id: Ulid::new(),
            };
        };
        if let Some(existing) = inner.podcasts.iter().find(|p| p.feed_url == feed_url) {
            return SubscribeResult::AlreadySubscribed {
                podcast_id: existing.id,
            };
        }
        let fallback_title = || feed_url.host_str().unwrap_or("podcast").to_string();
        let record = PodcastRecord {
            id: Ulid::new(),
            feed_url: feed_url.clone(),
            title: title.unwrap_or_else(fallback_title),
            author: author.unwrap_or_default(),
            artwork_url: None,
            subscribed_at_ms: now_ms(),
            last_refreshed_ms: None,
        };
        let id = record.id;
        inner.podcasts.push(record);
        SubscribeResult::Subscribed { podcast_id: id }
    }

    /// Drop a subscription. Idempotent — unknown ids return `false`.
    pub fn unsubscribe(&self, podcast_id: PodcastId) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let before = inner.podcasts.len();
        inner.podcasts.retain(|p| p.id != podcast_id);
        inner.podcasts.len() != before
    }

    /// Snapshot the current library as the
    /// `podcast_core::views::LibraryView` payload — what crosses FFI.
    pub fn snapshot(&self) -> LibraryView {
        let Ok(inner) = self.inner.lock() else {
            return LibraryView::default();
        };
        let podcasts = inner
            .podcasts
            .iter()
            .map(|record| PodcastRowPayload {
                id: record.id.to_string(),
                title: record.title.clone(),
                author: record.author.clone(),
                artwork_url: record.artwork_url.as_ref().map(|u| u.to_string()),
                episode_count: 0,
            })
            .collect();
        LibraryView { podcasts }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        s.parse().expect("valid url")
    }

    #[test]
    fn empty_snapshot_yields_empty_library() {
        let app = PodcastApp::new();
        let view = app.snapshot();
        assert!(view.podcasts.is_empty());
    }

    #[test]
    fn subscribe_then_snapshot_shows_podcast() {
        let app = PodcastApp::new();
        let res = app.subscribe(
            url("https://feeds.example.com/show.xml"),
            Some("My Show".into()),
            Some("Pablo".into()),
        );
        assert!(matches!(res, SubscribeResult::Subscribed { .. }));
        let view = app.snapshot();
        assert_eq!(view.podcasts.len(), 1);
        assert_eq!(view.podcasts[0].title, "My Show");
        assert_eq!(view.podcasts[0].author, "Pablo");
    }

    #[test]
    fn subscribe_dedupes_on_feed_url() {
        let app = PodcastApp::new();
        let first = app.subscribe(url("https://feeds.example.com/show.xml"), None, None);
        let second = app.subscribe(url("https://feeds.example.com/show.xml"), None, None);
        let SubscribeResult::Subscribed { podcast_id: id1 } = first else {
            panic!("first should be Subscribed");
        };
        let SubscribeResult::AlreadySubscribed { podcast_id: id2 } = second else {
            panic!("second should be AlreadySubscribed");
        };
        assert_eq!(id1, id2);
        assert_eq!(app.snapshot().podcasts.len(), 1);
    }

    #[test]
    fn unsubscribe_removes_podcast() {
        let app = PodcastApp::new();
        let res = app.subscribe(url("https://feeds.example.com/show.xml"), None, None);
        let SubscribeResult::Subscribed { podcast_id } = res else {
            panic!("subscribe failed");
        };
        assert!(app.unsubscribe(podcast_id));
        assert!(app.snapshot().podcasts.is_empty());
        // Idempotent: removing again returns false but does not panic.
        assert!(!app.unsubscribe(podcast_id));
    }

    #[test]
    fn fallback_title_uses_host_when_missing() {
        let app = PodcastApp::new();
        app.subscribe(url("https://feeds.example.com/show.xml"), None, None);
        let view = app.snapshot();
        assert_eq!(view.podcasts[0].title, "feeds.example.com");
    }
}
