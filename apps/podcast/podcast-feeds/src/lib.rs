// D0: feed-parsing nouns live here, never in nmp-core.
// Owns: RSS/Atom/JSON Feed parsing, Podcast Index API client, Podcasting 2.0 extensions.
// Reference Swift: RSSParser.swift, PodcastService.swift, PodcastIndexService.swift, Config.swift.
// Full implementation target: docs/design/podcast/podcast-feeds.md.

pub mod parser;
pub mod podcast_index;
pub mod podcasting20;

#[cfg(test)]
mod tests {
    use super::parser::{parse_feed, FeedError};

    #[test]
    fn podcast_feeds_parser_stub_returns_not_implemented() {
        let url = "https://feeds.example.com/podcast.rss".parse().unwrap();
        let result = parse_feed(b"<?xml version=\"1.0\"?>", &url);
        assert!(matches!(result, Err(FeedError::NotImplemented)));
    }

    #[test]
    fn podcast_feeds_podcast_index_types_serialize() {
        use super::podcast_index::IndexError;
        let err = IndexError::Auth;
        let s = format!("{err}");
        assert!(!s.is_empty());
    }

    #[test]
    fn podcast_feeds_podcasting20_default_is_empty() {
        use super::podcasting20::Podcasting20Extensions;
        let ext = Podcasting20Extensions::default();
        assert!(ext.persons.is_empty());
        assert!(ext.soundbites.is_empty());
        assert!(ext.transcript.is_none());
    }
}
