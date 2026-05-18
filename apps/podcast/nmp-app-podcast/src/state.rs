//! `PodcastApp` — the per-app projection state for the podcast library.
//!
//! Podcast records are backed by a kernel `DomainHandle` (in-memory
//! `MemEventStore` backend), replacing the previous `Vec<PodcastRecord>`
//! inside `Mutex<Inner>`. Episodes remain in-memory via
//! `HashMap<PodcastId, Vec<EpisodeRecord>>` — migration to a second
//! `DomainHandle` is deferred (T-podcast-gap-1 scope is podcasts only).
//!
//! ## Storage layout (podcasts)
//!
//! Namespace: `"podcast.podcasts"` (matches `PodcastsModule::NAMESPACE`).
//! Key: ULID bytes of `PodcastRecord::id` (16 bytes, lexicographically sortable).
//! Value: JSON-encoded `PodcastRecord`.
//!
//! ## HTTP-fetch gap (T-podcast-gap-3)
//!
//! This crate can parse feed bytes injected by the caller, but does NOT
//! perform HTTP fetching itself. The architecture requires the host platform
//! (Android OkHttp, iOS URLSession) to fetch the bytes and pass them in via
//! `ingest_feed_bytes()`. That capability boundary is tracked in:
//!   `docs/perf/m11/T-podcast-gap-3.md`
//!
//! D0: no podcast nouns in `nmp-core`. D6: every fallible path degrades
//! gracefully; no panics cross the FFI seam.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::store::{DomainHandle, EventStore, MemEventStore};
use podcast_core::domain::ids::PodcastId;
use podcast_core::domain::records::{DownloadState, EpisodeRecord, PodcastRecord};
use podcast_core::views::{EpisodeRowPayload, FeedView, LibraryView, PodcastRowPayload};
use podcast_feeds::parser::{self, FeedError, ParsedPodcast};
use ulid::Ulid;
use url::Url;

/// The key for a podcast record in the domain store — ULID bytes (16 bytes,
/// big-endian), which sorts chronologically and makes scan_prefix trivial.
fn ulid_key(id: PodcastId) -> [u8; 16] {
    id.to_bytes()
}

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

/// Per-app state.
///
/// Podcast records are backed by a `DomainHandle` (kernel domain store).
/// Episodes are held in-memory via a `Mutex<HashMap<PodcastId, Vec<EpisodeRecord>>>`.
pub struct PodcastApp {
    /// DomainHandle-backed podcast store.
    handle: DomainHandle,
    /// In-memory episode table. Populated by `ingest_feed_bytes`.
    episodes: Mutex<HashMap<PodcastId, Vec<EpisodeRecord>>>,
}

impl PodcastApp {
    pub fn new() -> Self {
        let store = MemEventStore::new();
        // MemEventStore::new() + domain_open() are infallible on a fresh store.
        let handle = store
            .domain_open("podcast.podcasts")
            .expect("MemEventStore domain_open is infallible");
        Self {
            handle,
            episodes: Mutex::new(HashMap::new()),
        }
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
        // Scan all records and check for an existing subscription on this URL.
        if let Some(record) = self.all_records().into_iter().find(|r| r.feed_url == feed_url) {
            return SubscribeResult::AlreadySubscribed {
                podcast_id: record.id,
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
        if let Ok(bytes) = serde_json::to_vec(&record) {
            // D6: ignore store errors — the state degrades gracefully.
            let _ = self.handle.put(&ulid_key(id), &bytes);
        }
        SubscribeResult::Subscribed { podcast_id: id }
    }

    /// Drop a subscription. Idempotent — unknown ids return `false`.
    /// Also removes the stored episode list for that podcast.
    pub fn unsubscribe(&self, podcast_id: PodcastId) -> bool {
        let removed = self.handle.delete(&ulid_key(podcast_id)).unwrap_or(false);
        if removed {
            if let Ok(mut eps) = self.episodes.lock() {
                eps.remove(&podcast_id);
            }
        }
        removed
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

        // Find the existing podcast record by feed_url.
        let mut record = match self
            .all_records()
            .into_iter()
            .find(|r| &r.feed_url == feed_url)
        {
            Some(r) => r,
            None => return IngestResult::PodcastNotFound,
        };

        // Update feed-level metadata from the parsed result.
        if !parsed.title.is_empty() {
            record.title = parsed.title;
        }
        if !parsed.author.is_empty() {
            record.author = parsed.author;
        }
        record.artwork_url = parsed.artwork_url;
        record.last_refreshed_ms = Some(now_ms());

        // Write updated record back to domain store.
        let podcast_id = record.id;
        if let Ok(bytes) = serde_json::to_vec(&record) {
            let _ = self.handle.put(&ulid_key(podcast_id), &bytes);
        }

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
        if let Ok(mut eps) = self.episodes.lock() {
            eps.insert(podcast_id, episodes);
        }

        IngestResult::Updated {
            podcast_id,
            episode_count,
        }
    }

    /// Snapshot the current library as `podcast_core::views::LibraryView`.
    /// `episode_count` reflects stored episodes for each podcast.
    pub fn snapshot(&self) -> LibraryView {
        let records = self.all_records();
        let eps = self.episodes.lock().ok();
        let podcasts = records
            .into_iter()
            .map(|record| {
                let episode_count = eps
                    .as_ref()
                    .and_then(|e| e.get(&record.id))
                    .map(|v| v.len() as u32)
                    .unwrap_or(0);
                PodcastRowPayload {
                    id: record.id.to_string(),
                    title: record.title,
                    author: record.author,
                    artwork_url: record.artwork_url.map(|u| u.to_string()),
                    episode_count,
                    feed_url: record.feed_url.to_string(),
                }
            })
            .collect();
        LibraryView { podcasts }
    }

    /// Return the episodes for a single podcast as `podcast_core::views::FeedView`.
    /// Unknown ids return an empty `FeedView` (honest empty state, not an error).
    pub fn episodes_for(&self, podcast_id: PodcastId) -> FeedView {
        let records = self.all_records();
        let podcast = records.iter().find(|p| p.id == podcast_id);
        let podcast_title = podcast.map(|p| p.title.as_str()).unwrap_or("").to_owned();
        let podcast_artwork = podcast
            .and_then(|p| p.artwork_url.as_ref())
            .map(|u| u.to_string());

        let Ok(eps) = self.episodes.lock() else {
            return FeedView::default();
        };
        let episodes = eps
            .get(&podcast_id)
            .map(|eps| {
                eps.iter()
                    .map(|ep| episode_to_payload(ep, &podcast_title, podcast_artwork.as_deref()))
                    .collect()
            })
            .unwrap_or_default();
        FeedView { episodes }
    }

    /// Read all `PodcastRecord` rows from the domain store. Silently skips
    /// rows that fail to deserialize (D6).
    fn all_records(&self) -> Vec<PodcastRecord> {
        let iter = match self.handle.scan_prefix(&[]) {
            Ok(it) => it,
            Err(_) => return vec![],
        };
        iter.filter_map(|row| {
            let (_, value) = row.ok()?;
            serde_json::from_slice(&value).ok()
        })
        .collect()
    }
}

impl Default for PodcastApp {
    fn default() -> Self {
        Self::new()
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
        pub_date_str: format_pub_date(ep.published_at_ms),
        download_state: format!("{:?}", ep.download_state),
        active_job_kind: None,
        has_insights: !ep.insight_ids.is_empty(),
        insights_count: ep.insight_ids.len() as u32,
        is_playing: false,
        // T-podcast-android-7: project the RSS enclosure URL so the Android
        // host can stream audio without any fabricated state on the Kotlin side.
        audio_url: ep.audio_url.to_string(),
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

const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format a millisecond Unix timestamp as "Mon D, YYYY".
/// Returns an empty string when `ms` is 0 (feed omitted the date).
fn format_pub_date(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    // Gregorian calendar from days since epoch (Euclidean algorithm, Neri/Schneidler).
    let days = (ms / 1000 / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe as i64 + era * 400 + if m <= 2 { 1 } else { 0 };
    let name = MONTH_NAMES[(m - 1) as usize];
    format!("{name} {d}, {y}")
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

    /// T-podcast-android-6: feed_url must roundtrip through the snapshot so
    /// the Android host can re-fetch bytes for pull-to-refresh without
    /// maintaining a separate URL index on the Kotlin side.
    #[test]
    fn snapshot_includes_feed_url_for_pull_to_refresh() {
        let app = PodcastApp::new();
        let feed = url("https://feeds.example.com/refresh-test.xml");
        app.subscribe(feed.clone(), Some("Refresh Test".into()), None);
        let view = app.snapshot();
        assert_eq!(view.podcasts.len(), 1);
        assert_eq!(
            view.podcasts[0].feed_url,
            "https://feeds.example.com/refresh-test.xml",
            "feed_url must roundtrip through the snapshot"
        );
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
    fn pub_date_str_is_non_empty_after_ingest_with_pubdate() {
        // rss2_one_episode has <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
        let feed_url = url("https://feeds.example.com/show.xml");
        let app = PodcastApp::new();
        let SubscribeResult::Subscribed { podcast_id } =
            app.subscribe(feed_url.clone(), Some("My Show".into()), None)
        else {
            panic!("subscribe failed");
        };
        app.ingest_feed_bytes(&feed_url, &rss2_one_episode());
        let feed_view = app.episodes_for(podcast_id);
        assert_eq!(feed_view.episodes.len(), 1);
        let ep = &feed_view.episodes[0];
        assert!(
            !ep.pub_date_str.is_empty(),
            "pub_date_str must be non-empty for an episode with a pubDate, got: {:?}",
            ep.pub_date_str,
        );
        // Jan 1, 2024 → "Jan 1, 2024"
        assert_eq!(ep.pub_date_str, "Jan 1, 2024");
    }

    #[test]
    fn pub_date_str_is_empty_when_no_pubdate() {
        // rss2_three_episodes has no <pubDate> tags
        let feed_url = url("https://feeds.example.com/multi.xml");
        let app = PodcastApp::new();
        let SubscribeResult::Subscribed { podcast_id } =
            app.subscribe(feed_url.clone(), None, None)
        else {
            panic!("subscribe failed");
        };
        app.ingest_feed_bytes(&feed_url, &rss2_three_episodes());
        let feed_view = app.episodes_for(podcast_id);
        for ep in &feed_view.episodes {
            assert!(
                ep.pub_date_str.is_empty(),
                "pub_date_str must be empty when feed omits pubDate, got: {:?}",
                ep.pub_date_str,
            );
        }
    }

    /// T-podcast-android-7: audio_url must roundtrip through the snapshot so
    /// the Android host can stream audio without any fabricated state on the
    /// Kotlin side. Uses the same enclosure URL from rss2_one_episode().
    #[test]
    fn episodes_for_roundtrips_audio_url() {
        let feed_url = url("https://feeds.example.com/show.xml");
        let app = PodcastApp::new();
        let SubscribeResult::Subscribed { podcast_id } =
            app.subscribe(feed_url.clone(), Some("Audio Test".into()), None)
        else {
            panic!("subscribe failed");
        };
        app.ingest_feed_bytes(&feed_url, &rss2_one_episode());
        let feed_view = app.episodes_for(podcast_id);
        assert_eq!(feed_view.episodes.len(), 1);
        assert_eq!(
            feed_view.episodes[0].audio_url,
            "https://example.com/ep1.mp3",
            "audio_url must roundtrip from RSS enclosure into EpisodeRowPayload"
        );
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
