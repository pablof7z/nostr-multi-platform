//! T142 unit tests — wire_frames_to_outbound conversion + role_for_relay_url.
//!
//! These pin the WireFrame→OutboundMessage bridge introduced in T142:
//! - Correct `RelayRole` is chosen from the kernel's relay-URL index.
//! - Unknown URLs fall back to `RelayRole::Content` (content lane accepts generic fetches).

#[cfg(test)]
mod tests {
    use crate::kernel::Kernel;
    use crate::planner::{InterestId, InterestLifecycle};
    use crate::relay::{RelayRole, BOOTSTRAP_DISCOVERY_RELAYS, DEFAULT_VISIBLE_LIMIT};
    use crate::subs::WireFrame;

    use super::super::outbound::wire_frames_to_outbound;

    // ─── T142-U5: known relay URL → correct RelayRole ────────────────────────

    /// When the relay URL matches the content bootstrap, the outbound message
    /// must carry `RelayRole::Content`.
    #[test]
    fn wire_frames_to_outbound_role_lookup() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        // Register the content bootstrap URL so role_for_relay_url can look it up.
        let content_url = BOOTSTRAP_DISCOVERY_RELAYS[0].to_string();
        kernel.relay_connecting(RelayRole::Content);

        let frame = WireFrame::Req {
            relay_url: content_url.clone(),
            sub_id: "t142-test-sub".to_string(),
            filter_json: r#"{"kinds":[1]}"#.to_string(),
            interest_id: InterestId(1),
            lifecycle: InterestLifecycle::Tailing,
        };

        let outbound = wire_frames_to_outbound(vec![frame], &mut kernel);
        assert_eq!(outbound.len(), 1);
        assert_eq!(outbound[0].role, RelayRole::Content);
        assert_eq!(outbound[0].relay_url, content_url);
    }

    // ─── T142-U6: unknown relay URL → RelayRole::Content fallback ────────────

    /// A relay URL not present in the kernel's role index must fall back to
    /// `RelayRole::Content` per spec §3.2 Option A.
    #[test]
    fn wire_frames_to_outbound_unknown_url_fallback() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let unknown_url = "wss://unknown-relay.example".to_string();

        let frame = WireFrame::Req {
            relay_url: unknown_url.clone(),
            sub_id: "t142-unknown-sub".to_string(),
            filter_json: r#"{"kinds":[1]}"#.to_string(),
            interest_id: InterestId(1),
            lifecycle: InterestLifecycle::Tailing,
        };

        let outbound = wire_frames_to_outbound(vec![frame], &mut kernel);
        assert_eq!(outbound.len(), 1);
        assert_eq!(
            outbound[0].role,
            RelayRole::Content,
            "unknown relay URL must fall back to RelayRole::Content",
        );
        assert_eq!(outbound[0].relay_url, unknown_url);
    }
}
