// D0: podcast nouns live in app modules like apps/podcast, never in nmp-core.
// This crate is the central domain crate for the podcast app.

pub mod actions;
pub mod domain;
pub mod views;

pub use domain::ids::*;
pub use domain::records::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podcast_core_domain_records_roundtrip() {
        let settings = SettingsRecord::default();
        let json = serde_json::to_string(&settings).expect("serialize settings");
        let back: SettingsRecord = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(settings, back);
    }

    #[test]
    fn podcast_core_action_stubs_serialize() {
        use actions::SubscribePodcast;
        let action = SubscribePodcast {
            feed_url: "https://feeds.example.com/podcast.rss".parse().unwrap(),
        };
        let json = serde_json::to_string(&action).expect("serialize action");
        assert!(json.contains("feed_url"));
    }
}
