//! `help [verb]` — one-line list or detailed grammar.

use crate::error::Result;

const SHORT: &str = "\
  verbs: set-seed, req, show, set-app-relays, set-indexer, set-dead,
         set-budget, refresh, expand, chirp, help, quit

  chirp: home, notifications, profile, thread, compose, reply, react,
         follow, unfollow, search, diagnostics, parity

  mls:   load-key, mls-init, mls-status, mls-create, mls-fetch-kp,
         mls-invite, mls-send, mls-accept, mls-messages

  variables: $me, $seed, $follows, $relays, $inbox
  type 'help <verb>' for grammar
";

const SET_SEED: &str = "\
  set-seed <nip05|npub|hex>
    Resolve the input to a 64-hex pubkey, clear follow + mailbox caches,
    update the prompt label.
    examples:
      set-seed _@f7z.io
      set-seed npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft
      set-seed fa984bd7dbb282f07e16e7ae87b26a2a7b9b9077b8a5d6c10d3c84d54f76d2a1
";

const REQ: &str = "\
  req <key=value>...
    Compile + fan out a filter against the live network. Variables expand
    at execute time. Required: at least one of kinds, authors, ids.

    filter fields:
      kinds=1,6          comma list of u32
      authors=$follows,<hex>,<npub>
      ids=<hex>,<hex>
      since=2026-01-01   or unix ts
      until=<date|ts>
      limit=200
      #t=bitcoin,nostr   single-letter tag filter

    examples:
      req kinds=1 authors=$follows
      req kinds=1 authors=$follows #t=bitcoin since=2026-01-01 limit=200
      req kinds=1 authors=$me
";

const SHOW: &str = "\
  show [state|relays|budget|seen]
    Dump current session state. Defaults to 'state'.
";

const SET_APP_RELAYS: &str = "\
  set-app-relays <url>[,<url>...]
    Override planner's app_relays fallback list. Default: empty.
";

const SET_INDEXER: &str = "\
  set-indexer <url>[,<url>...]
    Override discovery indexer set. Default: wss://purplepag.es.
";

const SET_DEAD: &str = "\
  set-dead <url>[,<url>...]
    Add relays to the dead set — skipped post-compile.
";

const SET_BUDGET: &str = "\
  set-budget [max_connections=N] [max_per_user=N] [wall=Ns]
    Patch outbox selector + fan-out wall budgets.
    wall accepts: 500ms, 20s, 2m.
";

const REFRESH: &str = "\
  refresh [follows|mailboxes|all]
    Invalidate caches. Default 'all'. Next req re-fetches.
";

const EXPAND: &str = "\
  expand $<var>
    Print the current expansion of a variable. Doesn't fetch anything;
    if the cache is empty for $follows / $relays, run `req` first.
";

const CHIRP: &str = "\
  chirp <surface> ...
    Feature-parity aliases for Chirp's app surfaces. Read surfaces drive
    the production planner + relay fanout; write surfaces sign with the
    loaded session key and publish to app relays.

    read / diagnostics:
      chirp home
      chirp notifications
      chirp profile <npub|nprofile|hex>
      chirp thread <note|nevent|hex>
      chirp search #nostr
      chirp diagnostics
      chirp parity

    write:
      chirp compose <text...>
      chirp reply <note|nevent|hex> <text...>
      chirp react <note|nevent|hex> [reaction]
      chirp follow <npub|nprofile|hex>
      chirp unfollow <npub|nprofile|hex>
";

const QUIT: &str = "\
  quit / exit
    Exit the REPL.
";

const LOAD_KEY: &str = "\
  load-key <nsec|hex>
    Import a secret key as the MLS identity for this session.
    Must be called before any mls-* command.
    examples:
      load-key nsec1...
      load-key fa984bd7...
";

const MLS_INIT: &str = "\
  mls-init
    Publish fresh key packages (kind:30443 + kind:443) to configured relays
    so peers can invite you. Requires load-key first.
";

const MLS_STATUS: &str = "\
  mls-status
    Show local MLS state: groups, pending welcomes, key-package cache.
";

const MLS_CREATE: &str = "\
  mls-create <name>
    Create a new MLS group with the given name.
    examples:
      mls-create \"my group\"
";

const MLS_FETCH_KP: &str = "\
  mls-fetch-kp <npub|hex>
    Fetch and cache key packages for a peer. Run before mls-invite.
    examples:
      mls-fetch-kp npub1...
";

const MLS_INVITE: &str = "\
  mls-invite [<group_id>] <npub|hex>
    Invite a peer into a group. Omit group_id to use the most recent group.
    Publishes a kind:1059 gift-wrap welcome to the peer.
    examples:
      mls-invite npub1...
      mls-invite abc123 npub1...
";

const MLS_SEND: &str = "\
  mls-send [<group_id>] <message>
    Encrypt and publish a kind:445 message to a group.
    examples:
      mls-send \"hello world\"
      mls-send abc123 \"hello world\"
";

const MLS_ACCEPT: &str = "\
  mls-accept <welcome_id|group_id>
    Accept a pending welcome and join the group.
";

const MLS_MESSAGES: &str = "\
  mls-messages [<group_id>]
    Print decrypted messages for a group.
";

pub fn run(arg: Option<String>) -> Result<()> {
    let text = match arg.as_deref() {
        None => SHORT,
        Some("set-seed") => SET_SEED,
        Some("req") => REQ,
        Some("show") => SHOW,
        Some("set-app-relays") => SET_APP_RELAYS,
        Some("set-indexer") => SET_INDEXER,
        Some("set-dead") => SET_DEAD,
        Some("set-budget") => SET_BUDGET,
        Some("refresh") => REFRESH,
        Some("expand") => EXPAND,
        Some("chirp") => CHIRP,
        Some("quit") | Some("exit") => QUIT,
        Some("load-key") => LOAD_KEY,
        Some("mls-init") => MLS_INIT,
        Some("mls-status") => MLS_STATUS,
        Some("mls-create") => MLS_CREATE,
        Some("mls-fetch-kp") => MLS_FETCH_KP,
        Some("mls-invite") => MLS_INVITE,
        Some("mls-send") => MLS_SEND,
        Some("mls-accept") => MLS_ACCEPT,
        Some("mls-messages") => MLS_MESSAGES,
        Some(other) => {
            println!("  (no help for '{other}'; type 'help' for the verb list)");
            return Ok(());
        }
    };
    print!("{text}");
    Ok(())
}
