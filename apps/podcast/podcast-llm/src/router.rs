// LLM route selector stub.
// Routes generation to Apple Intelligence (on-device) or rig.rs fallback.
// Reference: docs/design/podcast/podcast-llm.md §C.

use serde::{Deserialize, Serialize};

/// Which LLM backend to use for a generation request.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum LlmRoute {
    /// Apple FoundationModels on-device (iOS only, free, private).
    #[default]
    AppleIntelligence,
    /// rig.rs provider (cross-platform fallback).
    RigProvider {
        provider: RigProvider,
        model: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum RigProvider {
    OpenAi,
    Anthropic,
}

/// Stub: select the route based on platform + settings.
/// Real implementation reads SettingsRecord.llm_preferred_route.
pub fn select_route() -> LlmRoute {
    // Default to AppleIntelligence; runtime will downgrade if unavailable.
    LlmRoute::AppleIntelligence
}
