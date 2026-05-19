// D0: LLM action nouns live here, never in nmp-core.
// Owns: dual-path LLM router, summarize, extract-chapters, ask, guest-enrich,
//       excerpt-match, find-relevant-timestamp.
// Reference Swift: AIService.swift (308), GuestEnrichmentService.swift (94),
//                  InsightService.swift (233, matchExcerpt part).
// Full implementation target: docs/design/podcast/podcast-llm.md.

pub mod actions;
pub mod prompts;
pub mod router;

#[cfg(test)]
mod tests {
    use super::prompts::{EXTRACT_CHAPTERS_SYSTEM, SUMMARIZE_SYSTEM};
    use super::router::{select_route, LlmRoute};

    #[test]
    fn podcast_llm_default_route_is_apple_intelligence() {
        let route = select_route();
        assert_eq!(route, LlmRoute::AppleIntelligence);
    }

    #[test]
    fn podcast_llm_prompts_are_non_empty() {
        assert!(!SUMMARIZE_SYSTEM.is_empty());
        assert!(!EXTRACT_CHAPTERS_SYSTEM.is_empty());
    }
}
