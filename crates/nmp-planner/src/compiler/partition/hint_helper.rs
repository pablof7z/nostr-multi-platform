use crate::{
    interest::{HintSource, RelayHint, RelayUrl},
    plan::{HintOrigin, RoutingSource, UserConfiguredCategory},
};

/// Derive the `HintOrigin` that describes where this hint came from.
///
/// Parallel to `routing_source_for_hint` but captures the originating event id
/// when available, so the attribution record can surface it in diagnostics.
pub(super) fn hint_origin_for(hint: &RelayHint) -> HintOrigin {
    match &hint.source {
        HintSource::EventTag { event_id, .. } => HintOrigin::EventTag {
            event_id: event_id.clone(),
        },
        HintSource::Provenance { event_id } => HintOrigin::Provenance {
            event_id: event_id.clone(),
        },
        HintSource::UserConfigured => HintOrigin::UserConfigured,
    }
}

pub(super) fn route_for_hint(hint: &RelayHint) -> Option<(RelayUrl, RoutingSource)> {
    Some((
        canonical_hint_relay_url(&hint.url)?,
        routing_source_for_hint(&hint.source),
    ))
}

fn routing_source_for_hint(source: &HintSource) -> RoutingSource {
    match source {
        HintSource::EventTag { .. } => RoutingSource::Hint,
        HintSource::Provenance { .. } => RoutingSource::Provenance,
        HintSource::UserConfigured => RoutingSource::UserConfigured(UserConfiguredCategory::Debug),
    }
}

/// Canonicalize a relay hint URL through the single workspace authority
/// [`nmp_relay_url::canonicalize`] (fail-closed; the rules are NOT duplicated
/// here — #967). A hint the authority rejects yields `None`, so
/// [`route_for_hint`] drops it rather than routing to a malformed target.
fn canonical_hint_relay_url(raw: &str) -> Option<RelayUrl> {
    nmp_relay_url::canonicalize(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interest::{HintSource, RelayHint};

    fn hinted(url: &str) -> RelayHint {
        RelayHint {
            url: url.to_string(),
            source: HintSource::UserConfigured,
        }
    }

    #[test]
    fn canonicalizes_case_and_empty_path() {
        let (url, source) = route_for_hint(&hinted("  WSS://Relay.Ex/  ")).expect("valid");
        assert_eq!(url, "wss://relay.ex");
        assert_eq!(
            source,
            RoutingSource::UserConfigured(UserConfiguredCategory::Debug)
        );
    }

    #[test]
    fn rejects_missing_authority_and_non_ws_scheme() {
        assert!(route_for_hint(&hinted("wss:///path")).is_none());
        assert!(route_for_hint(&hinted("https://relay.example")).is_none());
    }
}
