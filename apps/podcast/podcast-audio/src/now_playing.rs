// NowPlayingViewModule payload stub.
// Drives MiniPlayer.swift and PlayerSheet.swift.
// Full reactivity: ADR-0002 coalesced ≤4 Hz audio-tick views.

use serde::{Deserialize, Serialize};

use crate::capability::PlaybackState;

/// Payload delivered to the Swift NowPlaying property wrapper.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct NowPlayingPayload {
    pub episode_id: Option<String>,
    pub podcast_id: Option<String>,
    pub title: String,
    pub podcast_title: String,
    pub artwork_url: Option<String>,
    pub progress_pct: f64,
    pub current_s: f64,
    pub duration_s: f64,
    pub state: PlaybackState,
}
