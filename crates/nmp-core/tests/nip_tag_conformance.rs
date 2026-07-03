//! NIP golden-tag conformance suite.
//!
//! A table of assertions that core-owned event kinds NMP *emits* carry exactly
//! the tags their NIP mandates — and no surprising tags besides.
//!
//! ## What this suite asserts, per emitted kind
//!
//! | Kind  | NIP     | Required tags                                          |
//! |-------|---------|--------------------------------------------------------|
//! | 1     | NIP-01  | top-level note: NO `e`/`p` tags                        |
//! | 1     | NIP-10  | reply: `e`(root) + `e`(reply) markers, `p`(parent)     |
//! | 3     | NIP-02  | one `p` per followed pubkey, nothing else              |
//! | 0     | NIP-01  | metadata: NO tags (content is JSON)                    |
//! | 23194 | NIP-47  | `p`(wallet pubkey)                                     |
//! | 10002 | NIP-65  | `r` per relay, optional `read`/`write` marker          |
//!
//! ## Robustness
//!
//! Tag arrays may appear in any order on the wire. Every assertion here checks
//! tags **by key**, never by position — `tags_with_key`, `p_values`,
//! `assert_only_keys`. The one ordering-sensitive property NIP-10 actually
//! mandates (root vs. reply `e` markers) is checked via the marker column, not
//! the array index.
//!
//! ## Driving the commands
//!
//! These tests reach the (crate-private) command handlers through the
//! `test-support` facade [`nmp_core::testing::ConformanceHarness`]. The target
//! only builds with `--features test-support`; verify with:
//!
//! ```text
//! cargo test -p nmp-core --features test-support --test nip_tag_conformance
//! ```
//!
//! ## Layout
//!
//! Each submodule below covers one NIP-tag-family scenario; `support` holds
//! the shared tag-inspection helpers and harness constructor they all use.

#[path = "nip_tag_conformance/support.rs"]
mod support;

#[path = "nip_tag_conformance/account_coldstart.rs"]
mod account_coldstart;
#[path = "nip_tag_conformance/cross_cutting.rs"]
mod cross_cutting;
#[path = "nip_tag_conformance/kind0_metadata.rs"]
mod kind0_metadata;
#[path = "nip_tag_conformance/kind10002_relay_list.rs"]
mod kind10002_relay_list;
#[path = "nip_tag_conformance/kind1_notes.rs"]
mod kind1_notes;
#[path = "nip_tag_conformance/kind3_contacts.rs"]
mod kind3_contacts;
