//! `PodcastApp` — the per-app projection state for the podcast library.
//!
//! Owns a `Mutex<Inner>` carrying subscribed podcasts and their episodes.
//! The episodes are stored keyed by `PodcastId` in a parallel `HashMap`
//! so the Library snapshot can reflect real `episode_count` values.
//!
//! ## HTTP-fetch gap (T-podcast-gap-3)
//!
//! This crate can parse feed bytes injected by the caller, but does NOT
//! perform HTTP fetching itself. The architecture requires the host platform
//! (Android OkHttp, iOS URLSession) to fetch the bytes and pass them in via
//! `ingest_feed_bytes()`. That capability boundary is tracked in:
//!   `docs/perf/m11/T-podcast-gap-3.md`
//!
//! Until that capability lands, the subscribe path stores only the feed URL
//! and title/author metadata. Once `ingest_feed_bytes` is called with real
//! feed bytes, the episodes table is populated.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use podcast_core::domain::ids::PodcastId;
use podcast_core::domain::records::{DownloadState, EpisodeRecord, PodcastRecord};
use podcast_core::views::{EpisodeRowPayload, FeedView, LibraryView, PodcastRowPayload};
use podcast_feeds::parser::{self, FeedError, ParsedPodcast};
use ulid::Ulid;
use url::Url;

/// Output of [`PodcastApp::subscribe`].
#[derive(Debug, Clone, PartialEq)]
pub enum SubscribeResult {
    Subscribed { podcast_id: PodcastId },
    AlreadySubscribed { podcast_id: PodcastId },
}

/// Result of [`PodcastApp::ingest_feed_bytes`].
#[derive(Debug, Clone, PartialEq)]
pub enum IngestResult {
    /// Episodes replaced/populated for the given podcast.
    Updated {
        podcast_id: PodcastId,
        episode_count: usize,
    },
    /// The feed URL was not found in the subscribed list.
    PodcastNotFound,
    /// Feed parsing failed — honest error, no fake episodes stored (D6).
    ParseError(String),
}

/// Per-app state. Holds the in-memory library and episode table.
#[derive(Default)]
pub struct PodcastApp {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Subscribed podcasts in subscription order.
    podcasts: Vec<PodcastRecord>,
    /// Episodes keyed by podcast id. Populated by `ingest_feed_bytes`.
    episodes: HashMap<PodcastId, Vec<EpisodeRecord>>,
}

impl PodcastApp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to a feed. Dedupes on `feed_url` — repeated calls return
    /// the existing `podcast_id`. Title/author are optional metadata; when
    /// missing we fall back to the host portion of the URL.
    pub fn subscribe(
        &self,
        feed_url: Url,
        title: Option<String>,
        author: Option<String>,
    ) -> SubscribeResult {
        let Ok(mut inner) = self.inner.lock() else {
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
    /// Also removes the stored episode list for that podcast.
    pub fn unsubscribe(&self, podcast_id: PodcastId) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let before = inner.podcasts.len();
        inner.podcasts.retain(|p| p.id != podcast_id);
        inner.episodes.remove(&podcast_id);
        inner.podcasts.len() != before
    }

    /// Ingest a parsed feed body for a given `feed_url`. The caller (host
    /// platform) is responsible for fetching the bytes (T-podcast-gap-3).
    ///
    /// - If `feed_url` is not subscribed, returns [`IngestResult::PodcastNotFound`].
    /// - If parsing fails, returns [`IngestResult::ParseError`] — never
    ///   stores fake episodes (D6).
    /// - On success, updates podcast metadata (title, author, artwork) and
    ///   replaces the episode table for that podcast.
    pub fn ingest_feed_bytes(&self, feed_url: &Url, bytes: &[u8]) -> IngestResult {
        let parsed: ParsedPodcast = match parser::parse_feed(bytes, feed_url) {
            Ok(p) => p,
            Err(FeedError::Invalid(msg)) => return IngestResult::ParseError(msg),
            Err(FeedError::Network(msg)) => return IngestResult::ParseError(msg),
        };

        let Ok(mut inner) = self.inner.lock() else {
            return IngestResult::PodcastNotFound;
        };

        let Some(podcast) = inner.podcasts.iter_mut().find(|p| &p.feed_url == feed_url) else {
            return IngestResult::PodcastNotFound;
        };

        // Update feed-level metadata from the parsed result.
        if !parsed.title.is_empty() {
            podcast.title = parsed.title;
        }
        if !parsed.author.is_empty() {
            podcast.author = parsed.author;
        }
        podcast.artwork_url = parsed.artwork_url;
        podcast.last_refreshed_ms = Some(now_ms());

        let podcast_id = podcast.id;

        // Convert parsed episodes to EpisodeRecord.
        let episodes: Vec<EpisodeRecord> = parsed
            .episodes
            .into_iter()
            .map(|ep| EpisodeRecord {
                id: Ulid::new(),
                podcast_id,
                guid: ep.guid,
                title: ep.title,
                ai_summary: None,
                description_text: ep.description,
                audio_url: ep.audio_url,
                duration_s: ep.duration_s,
                published_at_ms: ep.published_at_ms,
                download_state: DownloadState::NotDownloaded,
                local_audio_path: None,
                playback_position_s: 0.0,
                has_been_played: false,
                transcript_id: None,
                insight_ids: vec![],
                guest_ids: vec![],
            })
            .collect();

        let episode_count = episodes.len();
        inner.episodes.insert(podcast_id, episodes);

        IngestResult::Updated {
            podcast_id,
            episode_count,
        }
    }

    /// Snapshot the current library as `podcast_core::views::LibraryView`.
    /// `episode_count` reflects stored episodes for each podcast.
    pub fn snapshot(&self) -> LibraryView {
        let Ok(inner) = self.inner.lock() else {
            return LibraryView::default();
        };
        let podcasts = inner
            .podcasts
            .iter()
            .map(|record| {
                let episode_count = inner
                    .episodes
                    .get(&record.id)
                    .map(|eps| eps.len() as u32)
                    .unwrap_or(0);
                PodcastRowPayload {
                    id: record.id.to_string(),
                    title: record.title.clone(),
                    author: record.author.clone(),
                    artwork_url: record.artwork_url.as_ref().map(|u| u.to_string()),
                    episode_count,
                }
            })
            .collect();
        LibraryView { podcasts }
    }

    /// Return the episodes for a single podcast as `podcast_core::views::FeedView`.
    /// Unknown ids return an empty `FeedView` (honest empty state, not an error).
    pub fn episodes_for(&self, podcast_id: PodcastId) -> FeedView {
        let Ok(inner) = self.inner.lock() else {
            return FeedView::default();
        };
        let podcast = inner.podcasts.iter().find(|p| p.id == podcast_id);
        let podcast_title = podcast.map(|p| p.title.as_str()).unwrap_or("");
        let podcast_artwork = podcast
            .and_then(|p| p.artwork_url.as_ref())
            .map(|u| u.to_string());
        let episodes = inner
            .episodes
            .get(&podcast_id)
            .map(|eps| {
                eps.iter()
                    .map(|ep| {
                        episode_to_payload(ep, podcast_title, podcast_artwork.as_deref())
                    })
                    .collect()
            })
            .unwrap_or_default();
        FeedView { episodes }
    }
}

fn episode_to_payload(
    ep: &EpisodeRecord,
    podcast_title: &str,
    artwork_url: Option<&str>,
) -> EpisodeRowPayload {
    EpisodeRowPayload {
        id: ep.id.to_string(),
        title: ep.title.clone(),
        podcast_title: podcast_title.to_string(),
        podcast_artwork_url: artwork_url.map(|s| s.to_string()),
        summary: ep.ai_summary.clone().or_else(|| ep.description_text.clone()),
        duration_str: format_duration(ep.duration_s),
        download_state: format!("{:?}", ep.download_state),
        active_job_kind: None,
        has_insights: !ep.insight_ids.is_empty(),
        insights_count: ep.insight_ids.len() as u32,
        is_playing: false,
    }
}

fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
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

    fn rss2_one_episode() -> Vec<u8> {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Ingested Podcast</title>
    <description>A test</description>
    <item>
      <title>Episode One</title>
      <guid>ep-001</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
      <enclosure url="https://example.com/ep1.mp3" type="audio/mpeg" length="100"/>
      <description>First ep</description>
    </item>
  </channel>
</rss>"#
            .to_vec()
    }

    fn rss2_three_episodes() -> Vec<u8> {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Multi-Episode Podcast</title>
    <description>Three eps</description>
    <item>
      <title>Episode 1</title>
      <guid>ep-001</guid>
      <enclosure url="https://example.com/ep1.mp3" type="audio/mpeg" length="100"/>
    </item>
    <item>
      <title>Episode 2</title>
      <guid>ep-002</guid>
      <enclosure url="https://example.com/ep2.mp3" type="audio/mpeg" length="200"/>
    </item>
    <item>
      <title>Episode 3</title>
      <guid>ep-003</guid>
      <enclosure url="https://example.com/ep3.mp3" type="audio/mpeg" length="300"/>
    </item>
  </channel>
</rss>"#
            .to_vec()
    }

    #[test]
    fn empty_snapshot_yields_empty_library() {
        let app = PodcastApp::new();
        assert!(app.snapshot().podcasts.is_empty());
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
        assert_eq!(view.podcasts[0].episode_count, 0, "no episodes before ingest");
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
        assert!(!app.unsubscribe(podcast_id));
    }

    #[test]
    fn fallback_title_uses_host_when_missing() {
        let app = PodcastApp::new();
        app.subscribe(url("https://feeds.example.com/show.xml"), None, None);
        let view = app.snapshot();
        assert_eq!(view.podcasts[0].title, "feeds.example.com");
    }

    #[test]
    fn ingest_feed_bytes_populates_episodes_and_episode_count() {
        let feed_url = url("https://feeds.example.com/show.xml");
        let app = PodcastApp::new();
        let SubscribeResult::Subscribed { podcast_id } =
            app.subscribe(feed_url.clone(), Some("My Show".into()), None)
        else {
            panic!("subscribe failed");
        };

        let result = app.ingest_feed_bytes(&feed_url, &rss2_one_episode());
        assert!(
            matches!(result, IngestResult::Updated { episode_count: 1, .. }),
            "expected Updated with 1 episode, got {:?}",
            result
        );

        let view = app.snapshot();
        assert_eq!(
            view.podcasts[0].episode_count, 1,
            "episode_count must reflect ingested episodes"
        );

        let feed_view = app.episodes_for(podcast_id);
        assert_eq!(feed_view.episodes.len(), 1);
        assert_eq!(feed_view.episodes[0].title, "Episode One");
    }

    #[test]
    fn ingest_multiple_episodes_reflects_in_count() {
        let feed_url = url("https://feeds.example.com/multi.xml");
        let app = PodcastApp::new();
        app.subscribe(feed_url.clone(), Some("Multi".into()), None);

        let result = app.ingest_feed_bytes(&feed_url, &rss2_three_episodes());
        assert!(
            matches!(result, IngestResult::Updated { episode_count: 3, .. }),
            "expected 3 episodes, got {:?}",
            result
        );
        assert_eq!(app.snapshot().podcasts[0].episode_count, 3);
    }

    #[test]
    fn ingest_malformed_feed_returns_parse_error_no_fake_episodes() {
        let feed_url = url("https://feeds.example.com/bad.xml");
        let app = PodcastApp::new();
        app.subscribe(feed_url.clone(), None, None);

        let result = app.ingest_feed_bytes(&feed_url, b"NOT VALID XML OR JSON");
        assert!(
            matches!(result, IngestResult::ParseError(_)),
            "malformed feed must return ParseError, got {:?}",
            result
        );
        assert_eq!(app.snapshot().podcasts[0].episode_count, 0, "no fake episodes");
    }

    #[test]
    fn ingest_for_unknown_feed_url_returns_not_found() {
        let app = PodcastApp::new();
        let unknown = url("https://feeds.example.com/unknown.xml");
        let result = app.ingest_feed_bytes(&unknown, &rss2_one_episode());
        assert_eq!(result, IngestResult::PodcastNotFound);
    }

    #[test]
    fn ingest_blog_rss_yields_empty_episode_list_not_error() {
        let feed_url = url("https://feeds.example.com/blog.xml");
        let app = PodcastApp::new();
        app.subscribe(feed_url.clone(), None, None);

        let blog_rss = br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Blog</title>
    <item>
      <title>Post 1</title>
      <guid>post-1</guid>
      <link>https://example.com/post-1</link>
    </item>
  </channel>
</rss>"#;
        let result = app.ingest_feed_bytes(&feed_url, blog_rss);
        assert!(
            matches!(result, IngestResult::Updated { episode_count: 0, .. }),
            "blog feed with no audio must return Updated with 0, got {:?}",
            result
        );
        assert_eq!(app.snapshot().podcasts[0].episode_count, 0);
    }

    #[test]
    fn episodes_for_unknown_podcast_returns_empty_not_error() {
        let app = PodcastApp::new();
        let unknown_id = Ulid::new();
        let feed_view = app.episodes_for(unknown_id);
        assert!(feed_view.episodes.is_empty());
    }

    #[test]
    fn unsubscribe_also_removes_episodes() {
        let feed_url = url("https://feeds.example.com/show.xml");
        let app = PodcastApp::new();
        let SubscribeResult::Subscribed { podcast_id } =
            app.subscribe(feed_url.clone(), None, None)
        else {
            panic!("subscribe failed");
        };
        app.ingest_feed_bytes(&feed_url, &rss2_one_episode());
        assert_eq!(app.episodes_for(podcast_id).episodes.len(), 1);

        app.unsubscribe(podcast_id);
        assert!(app.episodes_for(podcast_id).episodes.is_empty());
    }
}
