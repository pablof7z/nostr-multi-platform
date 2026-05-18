// Feed parser — RSS 2.0, Atom, RDF, JSON Feed via `feed-rs`.
// Full implementation using feed-rs crate.
// Reference: docs/design/podcast/podcast-feeds.md §A.

use serde::{Deserialize, Serialize};
use url::Url;

/// Minimal parsed representation of a feed.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ParsedPodcast {
    pub title: String,
    pub author: String,
    pub feed_url: Url,
    pub artwork_url: Option<Url>,
    pub episodes: Vec<ParsedEpisode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ParsedEpisode {
    pub guid: String,
    pub title: String,
    pub audio_url: Url,
    pub duration_s: f64,
    pub published_at_ms: u64,
    pub description: Option<String>,
}

/// Parse RSS/Atom/JSON Feed bytes into a [`ParsedPodcast`].
///
/// On success returns the parsed podcast with all audio episodes.
/// On failure returns a [`FeedError`] describing what went wrong.
/// Malformed feeds and feeds that fail feed-rs parsing return honest errors
/// — never fake data. A feed with zero audio enclosures returns an empty
/// episode list (not an error).
pub fn parse_feed(bytes: &[u8], feed_url: &Url) -> Result<ParsedPodcast, FeedError> {
    let feed = feed_rs::parser::parse(bytes)
        .map_err(|e| FeedError::Invalid(e.to_string()))?;

    // Title — fall back to the URL host when the feed omits one.
    let title = feed
        .title
        .as_ref()
        .map(|t| t.content.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| feed_url.host_str().unwrap_or("podcast").to_string());

    // Author — feed-level author list (Person.name is String in feed-rs).
    let author = feed
        .authors
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_default();

    // Artwork — feed logo then icon.
    let artwork_url = feed
        .logo
        .as_ref()
        .and_then(|i| Url::parse(&i.uri).ok())
        .or_else(|| {
            feed.icon
                .as_ref()
                .and_then(|i| Url::parse(&i.uri).ok())
        });

    // Episodes — only entries with an audio enclosure become episodes.
    let episodes: Vec<ParsedEpisode> = feed
        .entries
        .into_iter()
        .filter_map(parse_episode)
        .collect();

    Ok(ParsedPodcast {
        title,
        author,
        feed_url: feed_url.clone(),
        artwork_url,
        episodes,
    })
}

/// Return true if a MediaTypeBuf looks like an audio media type.
fn is_audio_type(mt: &mediatype::MediaTypeBuf) -> bool {
    // .ty() / .subty() are methods on MediaTypeBuf (not fields).
    mt.ty() == mediatype::names::AUDIO
        || {
            let sub = mt.subty().as_str().to_lowercase();
            sub.contains("mpeg")
                || sub.contains("mp3")
                || sub.contains("ogg")
                || sub.contains("aac")
                || sub.contains("m4a")
                || sub.contains("opus")
        }
}

/// Return true if the URL path ends with an audio file extension.
fn has_audio_extension(url: &Url) -> bool {
    let path = url.path().to_lowercase();
    path.ends_with(".mp3")
        || path.ends_with(".m4a")
        || path.ends_with(".ogg")
        || path.ends_with(".aac")
        || path.ends_with(".opus")
}

/// Try to turn a feed entry into a `ParsedEpisode`. Returns `None` if the
/// entry has no audio enclosure (not a podcast episode).
fn parse_episode(entry: feed_rs::model::Entry) -> Option<ParsedEpisode> {
    // Find the first audio content item. RSS uses `<enclosure>`, Atom uses
    // `<link rel="enclosure">`. feed-rs normalises both to media objects.
    let audio_url: Option<Url> = entry
        .media
        .iter()
        .flat_map(|m| m.content.iter())
        .filter(|c| {
            c.content_type
                .as_ref()
                .map(is_audio_type)
                .unwrap_or(false)
        })
        .find_map(|c| c.url.as_ref().and_then(|u| Url::parse(u.as_str()).ok()))
        // Fallback 1: content with an audio-like URL extension.
        .or_else(|| {
            entry
                .media
                .iter()
                .flat_map(|m| m.content.iter())
                .find_map(|c| {
                    let u = c.url.as_ref()?;
                    let url = Url::parse(u.as_str()).ok()?;
                    if has_audio_extension(&url) { Some(url) } else { None }
                })
        })
        // Fallback 2: links with rel="enclosure".
        .or_else(|| {
            entry.links.iter().find_map(|l| {
                if l.rel.as_deref() == Some("enclosure") {
                    Url::parse(&l.href).ok()
                } else {
                    None
                }
            })
        });

    let audio_url = audio_url?;

    // entry.id is String (not Option<String>) in feed-rs.
    let guid = if entry.id.is_empty() {
        audio_url.to_string()
    } else {
        entry.id.clone()
    };

    let title = entry
        .title
        .as_ref()
        .map(|t| t.content.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| guid.clone());

    let published_at_ms = entry
        .published
        .or(entry.updated)
        .map(|dt| {
            let ts = dt.timestamp_millis();
            if ts < 0 { 0u64 } else { ts as u64 }
        })
        .unwrap_or(0);

    // Duration from media:content duration field — seconds as f64.
    let duration_s = entry
        .media
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|c| c.duration)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let description = entry
        .summary
        .as_ref()
        .map(|s| s.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            entry
                .content
                .as_ref()
                .and_then(|c| c.body.as_deref())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    Some(ParsedEpisode {
        guid,
        title,
        audio_url,
        duration_s,
        published_at_ms,
        description,
    })
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum FeedError {
    #[error("invalid feed: {0}")]
    Invalid(String),
    #[error("network error: {0}")]
    Network(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_url(s: &str) -> Url {
        s.parse().unwrap()
    }

    /// Minimal valid RSS 2.0 feed with one episode — happy path.
    fn rss2_minimal() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Test Podcast</title>
    <link>https://example.com</link>
    <description>A test podcast</description>
    <item>
      <title>Episode 1</title>
      <guid>ep-001</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 +0000</pubDate>
      <enclosure url="https://example.com/ep1.mp3" type="audio/mpeg" length="12345"/>
      <description>First episode</description>
    </item>
  </channel>
</rss>"#
    }

    /// RSS feed without a title — should fall back to URL host.
    fn rss2_no_title() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <link>https://feeds.example.com</link>
    <description>A test podcast</description>
    <item>
      <title>Episode 1</title>
      <guid>ep-001</guid>
      <enclosure url="https://example.com/ep1.mp3" type="audio/mpeg" length="100"/>
    </item>
  </channel>
</rss>"#
    }

    /// RSS feed with no enclosures — empty episode list expected.
    fn rss2_no_audio() -> &'static [u8] {
        br#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Blog Feed</title>
    <link>https://example.com</link>
    <description>A blog, not a podcast</description>
    <item>
      <title>Blog Post 1</title>
      <guid>post-001</guid>
      <link>https://example.com/post-1</link>
    </item>
  </channel>
</rss>"#
    }

    #[test]
    fn parse_rss2_minimal_success() {
        let url = feed_url("https://feeds.example.com/test.rss");
        let result = parse_feed(rss2_minimal(), &url);
        assert!(result.is_ok(), "should parse minimal RSS 2.0: {:?}", result.err());
        let podcast = result.unwrap();
        assert_eq!(podcast.title, "Test Podcast");
        assert_eq!(podcast.feed_url, url);
        assert_eq!(podcast.episodes.len(), 1, "expected 1 episode");
        let ep = &podcast.episodes[0];
        assert_eq!(ep.guid, "ep-001");
        assert_eq!(ep.title, "Episode 1");
        assert_eq!(ep.audio_url.as_str(), "https://example.com/ep1.mp3");
        assert_eq!(ep.description.as_deref(), Some("First episode"));
    }

    #[test]
    fn parse_rss2_missing_title_falls_back_to_host() {
        let url = feed_url("https://feeds.example.com/test.rss");
        let result = parse_feed(rss2_no_title(), &url);
        assert!(result.is_ok(), "should parse feed without title: {:?}", result.err());
        assert_eq!(result.unwrap().title, "feeds.example.com");
    }

    #[test]
    fn parse_rss2_no_audio_yields_empty_episode_list() {
        let url = feed_url("https://feeds.example.com/blog.rss");
        let result = parse_feed(rss2_no_audio(), &url);
        assert!(result.is_ok(), "should parse feed with no audio items: {:?}", result.err());
        assert!(
            result.unwrap().episodes.is_empty(),
            "items without audio enclosures must not become episodes"
        );
    }

    #[test]
    fn parse_malformed_feed_yields_error_not_panic() {
        let url = feed_url("https://feeds.example.com/bad.rss");
        let result = parse_feed(b"THIS IS NOT XML OR JSON", &url);
        assert!(result.is_err(), "malformed bytes must return Err");
        assert!(matches!(result.unwrap_err(), FeedError::Invalid(_)));
    }

    #[test]
    fn parse_empty_bytes_yields_error_not_panic() {
        let url = feed_url("https://feeds.example.com/empty.rss");
        let result = parse_feed(b"", &url);
        assert!(result.is_err(), "empty bytes must return Err, not a fake podcast");
    }

    #[test]
    fn parse_atom_feed_success() {
        let atom = br#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Podcast</title>
  <author><name>Atom Author</name></author>
  <id>urn:uuid:atom-podcast-1</id>
  <entry>
    <id>urn:uuid:entry-1</id>
    <title>Atom Episode 1</title>
    <updated>2024-01-01T00:00:00Z</updated>
    <link rel="enclosure" href="https://example.com/atom-ep1.mp3" type="audio/mpeg"/>
    <summary>First atom episode</summary>
  </entry>
</feed>"#;
        let url = feed_url("https://feeds.example.com/atom.xml");
        let result = parse_feed(atom, &url);
        assert!(result.is_ok(), "should parse Atom feed: {:?}", result.err());
        let podcast = result.unwrap();
        assert_eq!(podcast.title, "Atom Podcast");
        assert_eq!(podcast.author, "Atom Author");
        assert_eq!(podcast.episodes.len(), 1);
        assert_eq!(
            podcast.episodes[0].audio_url.as_str(),
            "https://example.com/atom-ep1.mp3"
        );
    }
}
