//! `nmp-nip87` — NIP-87 ecash mint discoverability event codecs.
//!
//! Owns the reusable protocol mechanics for [NIP-87]:
//!
//! - **kind:38172** — Cashu mint announcement (addressable; `d` = mint
//!   identifier). Advertises the mint's URLs, supported NUTs, and units so
//!   wallets can discover mints instead of hardcoding a list.
//! - **kind:38000** — mint recommendation / review (a user vouching for a
//!   mint), carrying a `k` tag naming the recommended announcement kind.
//!
//! Only **Cashu** (kind:38172) is in scope. NIP-87 also defines kind:38173 for
//! Fedimint; this crate deliberately does not model it, and
//! [`decode_mint_recommendation`] rejects recommendations whose `k` tag is not
//! `38172`.
//!
//! This crate is a thin adapter: pure parse/build over `nostr` primitives plus
//! NUT-capability parsing. It performs zero relay I/O and holds no product
//! policy. The app-facing "discovered / recommended mints" projection, the read
//! interests that subscribe to these kinds, and the web-of-trust-scoped,
//! fail-closed-on-missing-NUT aggregation live in `nmp-mint-discovery` (see
//! `docs/architecture/nip60-nip61-wallet-design.md`), which composes this
//! crate's codecs with `nmp-wot`'s trust scoring.
//!
//! [NIP-87]: https://github.com/nostr-protocol/nips/blob/master/87.md

pub mod announcement;
pub mod capabilities;
pub mod kinds;
pub mod ownership;
pub mod recommendation;

pub use announcement::{
    build_mint_announcement, decode_mint_announcement, decode_mint_announcement_event,
    mint_announcement_filter, MintAnnouncement,
};
pub use capabilities::{parse_capabilities, MintCapabilities, NUTZAP_REQUIRED_NUTS};
pub use kinds::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};
pub use recommendation::{
    build_mint_recommendation, decode_mint_recommendation, decode_mint_recommendation_event,
    mint_recommendation_filter, MintRecommendation,
};
