use std::collections::{BTreeMap, BTreeSet};

/// NIP-02 contact-list kind.
pub const KIND_CONTACT_LIST: u32 = 3;
/// NIP-51 public mute-list kind.
pub const KIND_MUTE_LIST: u32 = 10_000;

pub type Pubkey = String;

/// Summary of the graph size currently held in memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphStats {
    pub authors_with_follows: usize,
    pub authors_with_mutes: usize,
    pub follow_edges: usize,
    pub mute_edges: usize,
}

/// Result of ingesting an event-shaped signal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalIngest {
    Ignored,
    FollowList { author: Pubkey, follows: usize },
    MuteList { author: Pubkey, mutes: usize },
}

/// In-memory social signal graph.
///
/// Each author has at most one current follow list and one current public mute
/// list. Callers should feed replaceable events after their normal store has
/// resolved replacement semantics, or simply upsert newer lists here.
#[derive(Clone, Debug, Default)]
pub struct SignalGraph {
    follows: BTreeMap<Pubkey, BTreeSet<Pubkey>>,
    mutes: BTreeMap<Pubkey, BTreeSet<Pubkey>>,
}

impl SignalGraph {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert_follow_list<I, S>(&mut self, author: impl Into<Pubkey>, follows: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<Pubkey>,
    {
        let follows = follows
            .into_iter()
            .map(Into::into)
            .filter(|pk| is_hex_pubkey(pk))
            .collect::<BTreeSet<_>>();
        self.follows.insert(author.into(), follows);
    }

    pub fn upsert_mute_list<I, S>(&mut self, author: impl Into<Pubkey>, mutes: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<Pubkey>,
    {
        let mutes = mutes
            .into_iter()
            .map(Into::into)
            .filter(|pk| is_hex_pubkey(pk))
            .collect::<BTreeSet<_>>();
        self.mutes.insert(author.into(), mutes);
    }

    /// Ingest Nostr event fields. Only public `p` tags are consumed.
    ///
    /// Kind:3 updates the author's follow list. Kind:10000 updates the
    /// author's public mute list. Every other kind is ignored.
    pub fn ingest_event_tags(
        &mut self,
        kind: u32,
        author: &str,
        tags: &[Vec<String>],
    ) -> SignalIngest {
        if !is_hex_pubkey(author) {
            return SignalIngest::Ignored;
        }
        let pubkeys = p_tags(tags);
        match kind {
            KIND_CONTACT_LIST => {
                let count = pubkeys.len();
                self.upsert_follow_list(author.to_string(), pubkeys);
                SignalIngest::FollowList {
                    author: author.to_string(),
                    follows: count,
                }
            }
            KIND_MUTE_LIST => {
                let count = pubkeys.len();
                self.upsert_mute_list(author.to_string(), pubkeys);
                SignalIngest::MuteList {
                    author: author.to_string(),
                    mutes: count,
                }
            }
            _ => SignalIngest::Ignored,
        }
    }

    #[must_use]
    pub fn follows_of(&self, author: &str) -> Option<&BTreeSet<Pubkey>> {
        self.follows.get(author)
    }

    #[must_use]
    pub fn mutes_of(&self, author: &str) -> Option<&BTreeSet<Pubkey>> {
        self.mutes.get(author)
    }

    #[must_use]
    pub fn directly_follows(&self, viewer: &str, target: &str) -> bool {
        self.follows
            .get(viewer)
            .is_some_and(|follows| follows.contains(target))
    }

    #[must_use]
    pub fn directly_mutes(&self, viewer: &str, target: &str) -> bool {
        self.mutes
            .get(viewer)
            .is_some_and(|mutes| mutes.contains(target))
    }

    #[must_use]
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            authors_with_follows: self.follows.len(),
            authors_with_mutes: self.mutes.len(),
            follow_edges: self.follows.values().map(BTreeSet::len).sum(),
            mute_edges: self.mutes.values().map(BTreeSet::len).sum(),
        }
    }
}

fn p_tags(tags: &[Vec<String>]) -> Vec<Pubkey> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("p") {
                tag.get(1).filter(|pk| is_hex_pubkey(pk)).cloned()
            } else {
                None
            }
        })
        .collect()
}

#[must_use]
pub fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
