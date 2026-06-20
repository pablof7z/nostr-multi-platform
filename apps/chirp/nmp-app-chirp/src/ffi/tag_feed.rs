//! Chirp hashtag feed FFI.
//!
//! The shell dispatches only "open this tag" intent. This module owns the
//! NIP-12 tag normalization, primary note declaration, global interest scope,
//! compiled acquisition filter shape, and stable consumer id.

use std::ffi::{c_char, CString};

use nmp_ffi::{nmp_app_open_interest, NmpApp};

use super::helpers::c_string_opt;
use super::interest_feed::FEED_PRIMARY_KINDS;

/// Scope passed to `open_interest`: `1` = Global (account-agnostic). A hashtag
/// feed is not re-routed on account switch; it pins a public Nostr tag.
const SCOPE_GLOBAL: u32 = 1;

#[must_use]
fn tag_consumer(tag: &str) -> String {
    format!("tag-{tag}")
}

#[must_use]
fn normalize_tag(value: &str) -> Option<String> {
    let tag = value.trim().trim_start_matches('#').to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

#[must_use]
fn tag_feed_filter_json(tag: &str) -> Option<String> {
    let kinds = nmp_nip18::try_acquisition_kinds_for_primary(FEED_PRIMARY_KINDS)
        .ok()?
        .into_iter()
        .collect::<Vec<_>>();
    Some(serde_json::json!({ "kinds": kinds, "#t": [tag] }).to_string())
}

fn open_tag_interest(app: *mut NmpApp, tag: &str) {
    let Some(filter_json) = tag_feed_filter_json(tag) else {
        return;
    };
    let (Ok(filter), Ok(consumer)) = (CString::new(filter_json), CString::new(tag_consumer(tag)))
    else {
        return;
    };
    nmp_app_open_interest(app, filter.as_ptr(), consumer.as_ptr(), SCOPE_GLOBAL);
}

/// Open a global hashtag feed interest for primary kind:1 notes carrying the
/// normalized NIP-12 `#t` tag, with NIP-18 repost wrapper acquisition derived
/// from that primary declaration.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_chirp_open_tag_feed(app: *mut NmpApp, tag: *const c_char) {
    if app.is_null() {
        return;
    }
    let Some(tag) = c_string_opt(tag).and_then(|value| normalize_tag(&value)) else {
        return;
    };
    open_tag_interest(app, &tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_consumer_is_stable_and_namespaced() {
        assert_eq!(tag_consumer("nostr"), "tag-nostr");
    }

    #[test]
    fn tag_filter_json_carries_chirp_tag_policy() {
        assert_eq!(
            tag_feed_filter_json("nostr").unwrap(),
            r##"{"kinds":[1,6],"#t":["nostr"]}"##
        );
    }

    #[test]
    fn tag_filter_json_parses_as_interest_shape() {
        let json = tag_feed_filter_json("nostr").unwrap();
        assert!(
            nmp_planner::InterestShape::from_filter_json(&json).is_some(),
            "filter must parse: {json}"
        );
    }

    #[test]
    fn tag_feed_normalizes_user_input_in_app_ffi_layer() {
        assert_eq!(normalize_tag("  #Nostr  "), Some("nostr".to_string()));
        assert_eq!(normalize_tag("nostr"), Some("nostr".to_string()));
        assert_eq!(normalize_tag("###"), None);
    }
}
