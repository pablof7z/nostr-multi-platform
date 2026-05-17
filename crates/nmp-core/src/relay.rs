pub(crate) const CONTENT_RELAY_URL: &str = "wss://relay.primal.net";
pub(crate) const INDEXER_RELAY_URL: &str = "wss://purplepag.es";
pub(crate) const DEFAULT_VISIBLE_LIMIT: usize = 80;
pub(crate) const DEFAULT_EMIT_HZ: u32 = 4;
pub(crate) const TIMELINE_AUTHOR_LIMIT: usize = 500;
pub(crate) const PROFILE_REQ_BATCH: usize = 80;
pub(crate) const TEST_NPUB: &str =
    "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";
pub(crate) const TEST_PUBKEY: &str =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
pub(crate) const FIATJAF_PUBKEY: &str =
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
pub(crate) const JB55_PUBKEY: &str =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RelayRole {
    Content,
    Indexer,
}

impl RelayRole {
    pub(crate) fn all() -> [Self; 2] {
        [Self::Content, Self::Indexer]
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Indexer => "indexer",
        }
    }

    pub(crate) fn url(self) -> &'static str {
        match self {
            Self::Content => CONTENT_RELAY_URL,
            Self::Indexer => INDEXER_RELAY_URL,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OutboundMessage {
    pub(crate) role: RelayRole,
    pub(crate) text: String,
}
