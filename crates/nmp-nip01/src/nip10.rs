//! NIP-10 reference parsing and reply tag construction for NIP-01 notes.

use nmp_core::tags::{all_tag_values, e_tag, p_tag};
use serde::{Deserialize, Serialize};

/// A single `e`-tag reference: event id plus optional relay hint and marker.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

/// NIP-10 thread references decoded from a kind:1 event's tags.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip10Refs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_pubkeys: Vec<String>,
}

impl Nip10Refs {
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.root.is_none() && self.reply.is_none()
    }

    #[must_use]
    pub fn is_reply(&self) -> bool {
        self.reply.is_some()
    }
}

fn e_ref_from_tag(tag: &[String]) -> Option<EventRef> {
    let id = tag.get(1)?.clone();
    if id.is_empty() {
        return None;
    }
    let relay = tag.get(2).filter(|s| !s.is_empty()).cloned();
    let marker = tag.get(3).filter(|s| !s.is_empty()).cloned();
    Some(EventRef { id, relay, marker })
}

/// Parse NIP-10 thread references from raw tags.
#[must_use]
pub fn parse_nip10(tags: &[Vec<String>]) -> Nip10Refs {
    let e_tags: Vec<&Vec<String>> = tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("e"))
        .collect();

    let mentioned_pubkeys: Vec<String> = all_tag_values(tags, "p")
        .into_iter()
        .map(str::to_string)
        .collect();

    let has_marker = e_tags.iter().any(|t| {
        matches!(
            t.get(3).map(String::as_str),
            Some("root" | "reply" | "mention")
        )
    });

    if has_marker {
        let mut refs = Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        };
        for tag in &e_tags {
            let Some(eref) = e_ref_from_tag(tag) else {
                continue;
            };
            match eref.marker.as_deref() {
                Some("root") => {
                    if refs.root.is_none() {
                        refs.root = Some(eref);
                    }
                }
                Some("reply") => {
                    if refs.reply.is_none() {
                        refs.reply = Some(eref);
                    }
                }
                _ => refs.mentions.push(eref),
            }
        }
        if refs.reply.is_none() {
            refs.reply = refs.root.clone();
        }
        return refs;
    }

    let resolved: Vec<EventRef> = e_tags.iter().filter_map(|t| e_ref_from_tag(t)).collect();

    match resolved.len() {
        0 => Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        },
        1 => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[0].clone()),
            mentions: Vec::new(),
            mentioned_pubkeys,
        },
        n => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[n - 1].clone()),
            mentions: resolved[1..n - 1].to_vec(),
            mentioned_pubkeys,
        },
    }
}

/// Build the NIP-10 marked-form reply tag set.
#[must_use]
pub fn reply_tags(
    parent_id: &str,
    parent_author: &str,
    parent_refs: &Nip10Refs,
    relay_hint: Option<&str>,
) -> Vec<Vec<String>> {
    let (root_id, root_relay): (&str, Option<&str>) = match parent_refs.root.as_ref() {
        Some(root) => (root.id.as_str(), root.relay.as_deref()),
        None => (parent_id, relay_hint),
    };

    let mut pubkeys: Vec<&str> = Vec::with_capacity(1 + parent_refs.mentioned_pubkeys.len());
    pubkeys.push(parent_author);
    for pk in &parent_refs.mentioned_pubkeys {
        if !pubkeys.iter().any(|p| *p == pk.as_str()) {
            pubkeys.push(pk.as_str());
        }
    }

    let mut tags = Vec::with_capacity(2 + pubkeys.len());
    tags.push(e_tag(root_id, root_relay, Some("root")));
    tags.push(e_tag(parent_id, relay_hint, Some("reply")));
    for pk in pubkeys {
        tags.push(p_tag(pk, relay_hint));
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_root_and_reply() {
        let tags = vec![
            e_tag("ROOT", Some("wss://a"), Some("root")),
            e_tag("PARENT", Some("wss://b"), Some("reply")),
            vec!["p".into(), "author".into()],
        ];
        let r = parse_nip10(&tags);
        assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
        assert_eq!(r.root.as_ref().unwrap().relay.as_deref(), Some("wss://a"));
        assert_eq!(r.reply.as_ref().unwrap().id, "PARENT");
        assert!(r.is_reply());
        assert!(!r.is_root());
        assert_eq!(r.mentioned_pubkeys, vec!["author"]);
    }

    #[test]
    fn marked_root_only_makes_reply_equal_root() {
        let tags = vec![e_tag("ROOT", None, Some("root"))];
        let r = parse_nip10(&tags);
        assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
        assert_eq!(r.reply.as_ref().unwrap().id, "ROOT");
    }

    #[test]
    fn marked_mention_collected_separately() {
        let tags = vec![
            e_tag("ROOT", None, Some("root")),
            e_tag("PARENT", None, Some("reply")),
            e_tag("QUOTED", None, Some("mention")),
        ];
        let r = parse_nip10(&tags);
        assert_eq!(r.mentions.len(), 1);
        assert_eq!(r.mentions[0].id, "QUOTED");
    }

    #[test]
    fn positional_single_e_tag_is_root_and_reply() {
        let r = parse_nip10(&[vec!["e".into(), "ONLY".into()]]);
        assert_eq!(r.root.as_ref().unwrap().id, "ONLY");
        assert_eq!(r.reply.as_ref().unwrap().id, "ONLY");
        assert!(r.mentions.is_empty());
    }

    #[test]
    fn positional_three_e_tags_middle_is_mention() {
        let r = parse_nip10(&[
            vec!["e".into(), "ROOT".into()],
            vec!["e".into(), "MID".into()],
            vec!["e".into(), "PARENT".into()],
        ]);
        assert_eq!(r.root.as_ref().unwrap().id, "ROOT");
        assert_eq!(r.reply.as_ref().unwrap().id, "PARENT");
        assert_eq!(r.mentions.len(), 1);
        assert_eq!(r.mentions[0].id, "MID");
    }

    #[test]
    fn empty_e_tag_id_is_ignored() {
        let r = parse_nip10(&[vec!["e".into(), "".into()]]);
        assert!(r.is_root());
    }

    #[test]
    fn nip10refs_json_roundtrips_and_skips_empty() {
        let refs = Nip10Refs {
            root: Some(EventRef {
                id: "ROOT".into(),
                relay: None,
                marker: Some("root".into()),
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&refs).unwrap();
        assert!(!json.contains("mentions"));
        assert!(!json.contains("\"relay\""));
        let back: Nip10Refs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, refs);
    }

    #[test]
    fn reply_tags_for_mid_thread_note_inherits_root_ref() {
        let refs = Nip10Refs {
            root: Some(EventRef {
                id: "ROOT".into(),
                relay: Some("wss://r.root".into()),
                marker: Some("root".into()),
            }),
            mentioned_pubkeys: vec!["carol".into()],
            ..Default::default()
        };
        let tags = reply_tags("PARENT", "bob", &refs, None);
        assert_eq!(tags[0][1], "ROOT");
        assert_eq!(tags[0][2], "wss://r.root");
        assert_eq!(tags[1][1], "PARENT");
        assert_eq!(tags[2][1], "bob");
        assert_eq!(tags[3][1], "carol");
    }
}
