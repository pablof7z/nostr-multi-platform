//! Canonical NIP-65 relay-list tag decoding.
//!
//! This crate owns only the dependency-light wire-tag vocabulary for kind:10002:
//! turning `["r", <relay>, <marker?>]` tags into read/write/both relay sets.
//! Runtime ownership stays with `nmp-router`: it owns the mailbox cache, ingest
//! parser, publish action, and outbox resolver.

use nmp_kinds::KIND_RELAY_LIST;

/// Parsed kind:10002 relay-list tags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nip65RelayList {
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub both: Vec<String>,
}

impl Nip65RelayList {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read.is_empty() && self.write.is_empty() && self.both.is_empty()
    }

    /// Resolved read set: explicit reads plus unmarked bidirectional relays.
    #[must_use]
    pub fn read_set(&self) -> Vec<String> {
        let mut out = self.read.clone();
        out.extend(self.both.iter().cloned());
        out
    }

    /// Resolved write set: explicit writes plus unmarked bidirectional relays.
    #[must_use]
    pub fn write_set(&self) -> Vec<String> {
        let mut out = self.write.clone();
        out.extend(self.both.iter().cloned());
        out
    }
}

/// Parse NIP-65 relay-list tags for a known kind:10002 event.
#[must_use]
pub fn parse_relay_list_tags(tags: &[Vec<String>]) -> Nip65RelayList {
    let mut read = Vec::new();
    let mut write = Vec::new();
    let mut both = Vec::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("r") {
            continue;
        }
        let url = match tag.get(1) {
            Some(url) if url.starts_with("wss://") => match nmp_relay_url::canonicalize(url) {
                Some(canonical) => canonical,
                None => continue,
            },
            _ => continue,
        };
        match tag.get(2).map(String::as_str) {
            None | Some("") => both.push(url),
            Some("read") => read.push(url),
            Some("write") => write.push(url),
            Some(_) => {}
        }
    }

    sort_dedup(&mut read);
    sort_dedup(&mut write);
    sort_dedup(&mut both);

    Nip65RelayList { read, write, both }
}

/// Parse tags only when `kind` is NIP-65 kind:10002.
#[must_use]
pub fn parse_event_tags(kind: u32, tags: &[Vec<String>]) -> Option<Nip65RelayList> {
    (kind == KIND_RELAY_LIST).then(|| parse_relay_list_tags(tags))
}

fn sort_dedup(v: &mut Vec<String>) {
    v.sort();
    v.dedup();
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(url: &str, marker: Option<&str>) -> Vec<String> {
        let mut out = vec!["r".to_string(), url.to_string()];
        if let Some(marker) = marker {
            out.push(marker.to_string());
        }
        out
    }

    #[test]
    fn unmarked_r_tag_lands_in_both() {
        let parsed = parse_relay_list_tags(&[tag("wss://both.example", None)]);
        assert_eq!(parsed.read_set(), vec!["wss://both.example".to_string()]);
        assert_eq!(parsed.write_set(), vec!["wss://both.example".to_string()]);
    }

    #[test]
    fn marked_read_and_write_separate() {
        let parsed = parse_relay_list_tags(&[
            tag("wss://r.example", Some("read")),
            tag("wss://w.example", Some("write")),
            tag("wss://b.example", None),
        ]);

        assert!(parsed.read_set().contains(&"wss://r.example".to_string()));
        assert!(parsed.read_set().contains(&"wss://b.example".to_string()));
        assert!(!parsed.read_set().contains(&"wss://w.example".to_string()));
        assert!(parsed.write_set().contains(&"wss://w.example".to_string()));
        assert!(parsed.write_set().contains(&"wss://b.example".to_string()));
        assert!(!parsed.write_set().contains(&"wss://r.example".to_string()));
    }

    #[test]
    fn parse_event_tags_ignores_non_kind_10002() {
        assert_eq!(parse_event_tags(1, &[tag("wss://x.example", None)]), None);
    }

    #[test]
    fn non_wss_url_dropped() {
        let parsed = parse_relay_list_tags(&[
            tag("https://not-a-relay.example", None),
            tag("ws://insecure.example", None),
            tag("wss://ok.example", None),
        ]);
        assert_eq!(parsed.read_set(), vec!["wss://ok.example".to_string()]);
    }

    #[test]
    fn unknown_marker_ignored() {
        let parsed = parse_relay_list_tags(&[
            tag("wss://weird.example", Some("sideways")),
            tag("wss://ok.example", None),
        ]);
        assert!(!parsed
            .read_set()
            .contains(&"wss://weird.example".to_string()));
        assert!(parsed.read_set().contains(&"wss://ok.example".to_string()));
    }

    #[test]
    fn duplicate_urls_within_event_deduped() {
        let parsed =
            parse_relay_list_tags(&[tag("wss://x.example", None), tag("wss://x.example", None)]);
        assert_eq!(parsed.read_set(), vec!["wss://x.example".to_string()]);
    }

    #[test]
    fn canonicalizes_urls() {
        let parsed = parse_relay_list_tags(&[tag("wss://RELAY.EXAMPLE/", Some("write"))]);
        assert_eq!(parsed.write_set(), vec!["wss://relay.example".to_string()]);
    }

    #[test]
    fn rejects_hostless_wss_urls() {
        let parsed = parse_relay_list_tags(&[tag("wss://", None), tag("wss:///path", None)]);
        assert!(parsed.is_empty());
    }
}
