// D0: audio playback nouns live here, never in nmp-core.
// Owns: AudioPlaybackCapability definitions, NowPlayingViewModule, ad-skip policy.
// Reference Swift: AudioService.swift (323 LOC).
// Full implementation target: docs/design/podcast/podcast-core.md §D.3.

pub mod capability;
pub mod now_playing;

#[cfg(test)]
mod tests {
    use super::capability::{AudioCapabilityEvent, PlaybackState};
    use super::now_playing::NowPlayingPayload;

    #[test]
    fn podcast_audio_playback_state_default_is_idle() {
        let state = PlaybackState::default();
        assert_eq!(state, PlaybackState::Idle);
    }

    #[test]
    fn podcast_audio_now_playing_default_is_empty() {
        let payload = NowPlayingPayload::default();
        assert!(payload.episode_id.is_none());
        assert_eq!(payload.progress_pct, 0.0);
    }

    #[test]
    fn podcast_audio_capability_event_serializes() {
        let event = AudioCapabilityEvent::Tick { current_s: 42.5, duration_s: 3600.0 };
        let json = serde_json::to_string(&event).expect("serialize tick event");
        assert!(json.contains("42.5"));
    }
}
