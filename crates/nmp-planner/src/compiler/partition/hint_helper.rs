use crate::{
    interest::{HintSource, RelayHint, RelayUrl},
    plan::{RoutingSource, UserConfiguredCategory},
};

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

fn canonical_hint_relay_url(raw: &str) -> Option<RelayUrl> {
    let s = raw.trim();
    let sep = s.find("://")?;
    let scheme = s[..sep].to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return None;
    }
    let rest = &s[sep + 3..];
    if rest.is_empty() {
        return None;
    }
    let (authority, path_etc) = if let Some(pos) = rest.find(['/', '?', '#']) {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };
    if authority.is_empty() {
        return None;
    }
    let path_etc_norm = if path_etc == "/" { "" } else { path_etc };
    Some(format!(
        "{scheme}://{}{path_etc_norm}",
        authority.to_ascii_lowercase()
    ))
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
