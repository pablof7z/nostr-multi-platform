use std::collections::BTreeSet;

pub(crate) fn derived_kinds<'a>(ids: impl IntoIterator<Item = &'a str>) -> Option<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut kinds = Vec::new();
    for id in ids {
        let kind = kind_for_id(id)?;
        if seen.insert(kind.clone()) {
            kinds.push(kind);
        }
    }
    Some(kinds)
}

pub(crate) fn kind_for_id(value: &str) -> Option<String> {
    if !valid_external_id_chars(value) {
        return None;
    }
    if is_web_external_id(value) {
        return Some("web".to_string());
    }
    if prefixed_payload(value, "#").is_some() {
        return Some("#".to_string());
    }
    if prefixed_payload(value, "isbn:").is_some_and(valid_isbn_payload) {
        return Some("isbn".to_string());
    }
    if prefixed_payload(value, "geo:").is_some_and(valid_geo_payload) {
        return Some("geo".to_string());
    }
    if prefixed_payload(value, "iso3166:").is_some_and(valid_iso3166_payload) {
        return Some("iso3166".to_string());
    }
    for (prefix, kind) in [
        ("podcast:item:guid:", "podcast:item:guid"),
        ("podcast:publisher:guid:", "podcast:publisher:guid"),
        ("podcast:guid:", "podcast:guid"),
        ("isan:", "isan"),
        ("doi:", "doi"),
    ] {
        if prefixed_payload(value, prefix).is_some() {
            return Some(kind.to_string());
        }
    }
    blockchain_kind_for_id(value)
}

fn valid_external_id_chars(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
}

fn is_web_external_id(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest
        .split(|ch| matches!(ch, '/' | '?' | '#'))
        .next()
        .unwrap_or_default();
    !host.is_empty()
}

fn prefixed_payload<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .strip_prefix(prefix)
        .filter(|payload| !payload.is_empty())
}

fn valid_isbn_payload(payload: &str) -> bool {
    payload
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'X' | b'x'))
}

fn valid_geo_payload(payload: &str) -> bool {
    payload
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn valid_iso3166_payload(payload: &str) -> bool {
    payload
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

fn blockchain_kind_for_id(value: &str) -> Option<String> {
    let parts: Vec<&str> = value.split(':').collect();
    let (chain, selector, external_id) = match parts.as_slice() {
        [chain, selector, external_id] if matches!(*selector, "tx" | "address") => {
            (*chain, *selector, *external_id)
        }
        [chain, chain_id, selector, external_id]
            if !chain_id.is_empty() && matches!(*selector, "tx" | "address") =>
        {
            (*chain, *selector, *external_id)
        }
        _ => return None,
    };
    if chain.is_empty()
        || external_id.is_empty()
        || !chain
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return None;
    }
    Some(format!("{chain}:{selector}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_nip73_external_ids_derive_expected_kinds() {
        let cases = [
            ("http://example.com/article", "web"),
            ("https://example.com/article", "web"),
            ("isbn:9780765382030", "isbn"),
            ("geo:ezs42e44yx96", "geo"),
            ("iso3166:US-CA", "iso3166"),
            ("isan:0000-0000-401A-0000-7", "isan"),
            ("doi:10.1000/xyz123", "doi"),
            ("#nostr", "#"),
            (
                "podcast:guid:c90e609a-df1e-596a-bd5e-57bcc8aad6cc",
                "podcast:guid",
            ),
            (
                "podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f",
                "podcast:item:guid",
            ),
            (
                "podcast:publisher:guid:18bcbf10-6701-4ffb-b255-bc057390d738",
                "podcast:publisher:guid",
            ),
            (
                "bitcoin:tx:a1075db55d416d3ca199f55b6084e2115b9345e16c5cf302fc80e9d5fbf5d48d",
                "bitcoin:tx",
            ),
            (
                "bitcoin:address:1HQ3Go3ggs8pFnXuHVHRytPCq5fGG8Hbhx",
                "bitcoin:address",
            ),
            (
                "ethereum:1:tx:0x98f7812be496f97f80e2e98d66358d1fc733cf34176a8356d171ea7fbbe97ccd",
                "ethereum:tx",
            ),
            (
                "ethereum:100:address:0xd8da6bf26964af9d7eed9e03e53415D37aA96045",
                "ethereum:address",
            ),
        ];

        for (id, kind) in cases {
            assert_eq!(kind_for_id(id).as_deref(), Some(kind), "{id}");
        }
    }

    #[test]
    fn malformed_external_ids_are_rejected() {
        for external_id in [
            "",
            "https://",
            "podcast:item:guid:",
            "not-a-nip73-id",
            "doi:bad value",
            "doi:bad\nvalue",
            "geo:EZS42E44YX96",
            "iso3166:us-ca",
            "bitcoin:tx:",
            "ethereum::tx:abc",
            "bitcoin:block:abc",
        ] {
            assert_eq!(kind_for_id(external_id), None, "{external_id:?}");
        }
    }

    #[test]
    fn derived_kinds_are_deduped_in_first_seen_order() {
        assert_eq!(
            derived_kinds([
                "https://example.com/one",
                "http://example.com/two",
                "isbn:9780765382030",
            ]),
            Some(vec!["web".to_string(), "isbn".to_string()])
        );
    }
}
