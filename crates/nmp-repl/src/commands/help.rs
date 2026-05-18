//! `help [verb]` — one-line list or detailed grammar.

use crate::error::Result;

const SHORT: &str = "\
  verbs: set-seed, req, show, set-app-relays, set-indexer, set-dead,
         set-budget, refresh, expand, help, quit
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

const QUIT: &str = "\
  quit / exit
    Exit the REPL.
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
        Some("quit") | Some("exit") => QUIT,
        Some(other) => {
            println!("  (no help for '{other}'; type 'help' for the verb list)");
            return Ok(());
        }
    };
    print!("{text}");
    Ok(())
}
