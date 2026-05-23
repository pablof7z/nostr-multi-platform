//! Pure AST types for parsed REPL commands. No I/O, no session reads.
//!
//! See `docs/design/nmp-repl.md` §5.

use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    SetSeed(SeedInput),
    Req(FilterAst),
    Show(ShowTopic),
    SetAppRelays(Vec<String>),
    SetIndexer(Vec<String>),
    SetDead(Vec<String>),
    SetBudget(BudgetPatch),
    Refresh(RefreshScope),
    Expand(VarName),
    Help(Option<String>),

    // ── MLS / Marmot (bypass-kernel, direct-WebSocket) ───────────────────
    /// `create-account [name] [relay…]` — generate keys, publish kind:0 + kind:10002.
    /// Relay URLs are set as `app_relays` for the session before account creation.
    /// Gated behind the `mls` feature because it writes to the network.
    #[cfg(feature = "mls")]
    CreateAccount(Option<String>, Vec<String>),
    /// `load-key <nsec|hex>` — adopt an existing identity.
    LoadKey(String),
    /// `mls-init` — build the in-memory MarmotService, publish KeyPackages.
    #[cfg(feature = "mls")]
    MlsInit,
    /// `mls-status` — snapshot groups / welcomes / key-package cache.
    #[cfg(feature = "mls")]
    MlsStatus,
    /// `mls-create <name>` — create a solo MLS group.
    #[cfg(feature = "mls")]
    MlsCreate(String),
    /// `mls-fetch-kp <npub>` — fetch + cache a peer's KeyPackage.
    #[cfg(feature = "mls")]
    MlsFetchKp(String),
    /// `mls-invite <group_hex> <npub>` — add a member + send the Welcome.
    #[cfg(feature = "mls")]
    MlsInvite(String, String),
    /// `mls-accept [welcome_hex]` — accept a pending Welcome (or list them).
    #[cfg(feature = "mls")]
    MlsAccept(Option<String>),
    /// `mls-send <group_hex> <text...>` — encrypt + publish a group message.
    #[cfg(feature = "mls")]
    MlsSend(String, String),
    /// `mls-messages <group_hex>` — print decrypted message history.
    #[cfg(feature = "mls")]
    MlsMessages(String),

    Quit,
    /// Empty line — no-op.
    Noop,
}

/// Seed input form (pre-resolution); the executor normalises to hex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedInput {
    /// `name@domain` form.
    Nip05(String),
    /// bech32 `npub1...`.
    Npub(String),
    /// 64-hex pubkey.
    Hex(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShowTopic {
    State,
    Relays,
    Budget,
    Seen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshScope {
    Follows,
    Mailboxes,
    All,
}

/// A variable reference (e.g. `$follows`). Stored without the leading `$`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarName(pub String);

/// A filter-field value — either a literal token or a `$var` reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Lit(String),
    Var(String),
}

/// Parsed `req` filter shape. All fields are optional in parse-shape;
/// `req`'s executor validates the "at least one of kinds/authors/ids" rule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterAst {
    pub kinds: Option<Vec<u32>>,
    pub authors: Option<Vec<Value>>,
    pub ids: Option<Vec<Value>>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: Option<u32>,
    pub tags: BTreeMap<char, Vec<Value>>,
}

/// Partial budget update — only fields the user named are `Some`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BudgetPatch {
    pub max_connections: Option<usize>,
    pub max_per_user: Option<usize>,
    pub wall: Option<Duration>,
}
