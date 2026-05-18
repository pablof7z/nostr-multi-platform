// AudioPlaybackCapability stub.
// The capability bridge lives in the iOS shell; this crate defines the contract.
// Reference: docs/design/podcast/capabilities.md.

use serde::{Deserialize, Serialize};

/// Current playback state from the native AudioPlaybackCapability.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum PlaybackState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Error(String),
}

/// Events emitted by AudioPlaybackCapability on the capability event bus.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum AudioCapabilityEvent {
    StateChanged(PlaybackState),
    /// Emitted at ≤4 Hz while playing (D8: coalesced, not per-frame).
    Tick { current_s: f64, duration_s: f64 },
    Error { reason: String },
}

/// Requests sent to the capability from action modules.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum AudioCapabilityRequest {
    Load { url_or_path: String, start_s: Option<f64> },
    Play,
    Pause,
    Resume,
    Seek { to_s: f64 },
    SetRate { rate: f32 },
    Stop,
}
