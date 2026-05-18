# `nmp-repl` — diagnostic REPL for the NMP Nostr framework

**Status.** v1 implementation plan. Hand to one engineer; ship in 1–2 days.
**Scope.** Single new workspace crate, single new binary `nmp-repl`.
**Read-only.** No publishes, no AUTH, no NIP-77. Diagnostic only.
**Reference impl.** `crates/nmp-core/examples/outbox_perf.rs` is the working
proof for every non-trivial moving part (compiler integration, kind:3 and
kind:10002 fetch, parallel relay fan-out, mailbox parsing). The REPL is
fundamentally that example, rearranged behind a line editor with a live
renderer.

---

## 1 — TL;DR

`nmp-repl` is an interactive terminal that compiles and executes Nostr
subscription plans against the live network by driving the **production
`nmp_core::subs::SubscriptionLifecycle`** — the exact code path the app
uses. The REPL does *not* reimplement the outbox: `recompile_and_diff`
performs implicit kind:10002 discovery, NIP-65 routing, the dead-relay
filter, and `apply_selection` internally; the REPL only ticks it and fans
out the `WireFrame::Req`s it produces. **This is the whole point — we test
real code, not a parallel reimplementation.** Operators type filter-shaped
commands like `req kinds=1 authors=$follows`; the REPL resolves variables
(kind:3 follows is a thin targeted fetch — variable resolution, not outbox),
drives the lifecycle's discovery-convergence tick loop, then fans the
converged content plan out to the resulting relay set on bounded worker
threads and streams a live per-relay status table back to the terminal.
Built for diagnosing outbox correctness, relay coverage, and
unroutable-author surfaces — not for end-user reading.

---

## 2 — Crate layout

New workspace member at `crates/nmp-repl/`. Binary name `nmp-repl`. Library
modules organised so each owns one concern and can be unit-tested in
isolation.

```
crates/nmp-repl/
├── Cargo.toml
└── src/
    ├── main.rs          # arg parse, install rustls provider, build Session, run loop
    ├── session.rs       # Session struct + all REPL state (seed, caches, budgets)
    ├── parser.rs        # command tokenizer + filter parser (AST only, no execution)
    ├── ast.rs           # Command / FilterAst / Value enums (no logic)
    ├── nip05.rs         # NIP-05 resolver (ureq + serde_json)
    ├── ws.rs            # tungstenite helpers — connect, next_text, normalize_url
    ├── discovery.rs     # kind:3 fetch, kind:10002 fetch, MailboxSnapshot build
    ├── plan.rs          # wraps SubscriptionCompiler::with_relays + apply_selection
    ├── fanout.rs        # bounded worker pool; one std::thread per relay
    ├── render.rs        # crossterm-based live status table (between readlines)
    ├── commands/        # one verb per file; each is a small fn over &mut Session
    │   ├── mod.rs
    │   ├── set_seed.rs
    │   ├── req.rs
    │   ├── show.rs
    │   ├── set_app_relays.rs
    │   ├── set_indexer.rs
    │   ├── set_budget.rs
    │   ├── refresh.rs
    │   ├── expand.rs
    │   └── help.rs
    └── error.rs         # ReplError enum, Display impl, exit codes
```

No `lib.rs` — this crate is a binary only. If we ever want to embed it,
promote modules to a library at that point.

---

## 3 — Command grammar (v1)

Tokens are whitespace-separated. Tokens of the form `key=val` are filter
fields (only valid inside `req`); bare tokens are positional. Variables
are tokens beginning with `$`. Comma is the in-value list separator.

| Verb              | Args                                       | Effect                                                                                  |
|-------------------|--------------------------------------------|-----------------------------------------------------------------------------------------|
| `set-seed`        | `<nip05>` \| `<npub>` \| `<hex>`           | Resolve to hex pubkey; clear follow + mailbox caches; prompt updates                    |
| `req`             | `<filter-fields>...`                       | Discover → compile → fanout. Live render. Updates seen-id set.                          |
| `show`            | `state` \| `relays` \| `budget` \| `seen`  | Dump current session state                                                              |
| `set-app-relays`  | `<url>[,<url>...]`                         | Override the `app_relays` fallback list (default empty)                                 |
| `set-indexer`     | `<url>[,<url>...]`                         | Override the indexer set used for kind:3 / kind:10002 (default `wss://purplepag.es`)    |
| `set-dead`        | `<url>[,<url>...]`                         | Mark relays as dead — skipped by the compiler/fanout (default empty)                    |
| `set-budget`      | `max_connections=N` `max_per_user=N` `wall=Ns` | Adjust selector + fanout wall                                                       |
| `refresh`         | `[follows \| mailboxes \| all]`            | Drop the named cache; next `req` re-fetches. Default `all`.                             |
| `expand`          | `<$var>`                                   | Print the current expansion of a variable                                               |
| `help`            | `[<verb>]`                                 | One-line list; with arg, detailed grammar for that verb                                 |
| `quit` / `exit`   | —                                          | Exit cleanly                                                                            |

### 3.1 Examples

```
> set-seed _@f7z.io
> req kinds=1 authors=$follows since=2026-01-01 limit=200
> req kinds=1 authors=$follows #t=bitcoin,nostr
> set-budget max_connections=50 max_per_user=3 wall=30s
> set-indexer wss://purplepag.es,wss://relay.nostr.band
> refresh mailboxes
> show relays
> expand $follows
```

### 3.2 Filter-field keys

| Key            | Values                                              | Notes                                       |
|----------------|-----------------------------------------------------|---------------------------------------------|
| `kinds`        | comma list of `u32`                                 | Required for `req`                          |
| `authors`      | comma list of hex pubkeys, npubs, or `$<var>`       | Variables expand at execute-time            |
| `ids`          | comma list of hex event ids                         | —                                           |
| `since`        | unix ts or `YYYY-MM-DD`                             | Inclusive lower bound                       |
| `until`        | unix ts or `YYYY-MM-DD`                             | Exclusive upper bound                       |
| `limit`        | `u32`                                               | Per-relay limit                             |
| `#<letter>`    | comma list of strings                               | Single-letter tag filter (NIP-01 §filter)   |

Unknown keys are a parse error. Mixing key=val tokens with bare tokens in
`req` is a parse error.

---

## 4 — Variables

Expanded at execute-time, not parse-time. Using a variable before its
dependency is resolved produces a clear runtime error pointing at the
missing command (e.g. `"$follows requires a seed; run `set-seed` first"`).

| Var        | Resolves to                                                                                                | Cache key       |
|------------|------------------------------------------------------------------------------------------------------------|-----------------|
| `$me`      | Session seed hex pubkey                                                                                    | none            |
| `$seed`    | Alias for `$me`                                                                                            | none            |
| `$follows` | The seed's kind:3 `p`-tag set (hex)                                                                        | `follows_cache` |
| `$relays`  | The seed's kind:10002 write relays                                                                         | `mailbox_cache` |
| `$inbox`   | The seed's kind:10002 read relays                                                                          | `mailbox_cache` |

`$follows` and `$relays` trigger discovery the first time they're used and
the cache is empty (or `refresh` was called).

---

## 5 — Filter parser

Lives in `src/parser.rs`; emits an AST defined in `src/ast.rs`. Pure
function — no I/O, no session reads.

```rust
// ast.rs
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
    Quit,
}

pub enum SeedInput { Nip05(String), Npub(String), Hex(String) }

pub struct FilterAst {
    pub kinds: Option<Vec<u32>>,
    pub authors: Option<Vec<Value>>,   // Value::Lit | Value::Var
    pub ids: Option<Vec<Value>>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub limit: Option<u32>,
    pub tags: BTreeMap<char, Vec<Value>>,
}

pub enum Value { Lit(String), Var(String) }

pub struct BudgetPatch {
    pub max_connections: Option<usize>,
    pub max_per_user:    Option<usize>,
    pub wall:            Option<Duration>,
}
```

### 5.1 Tokenizer

Whitespace split (no quoting in v1 — filter values are never spaces).
Reject control chars. Empty input → no-op.

### 5.2 Filter-field grammar (regex-shaped)

```
field   := key "=" value
key     := "kinds" | "authors" | "ids" | "since" | "until" | "limit"
         | "#" /[a-zA-Z]/
value   := atom ("," atom)*
atom    := /[A-Za-z0-9._:@/+-]+/   ; conservative; rejects spaces & '='
         | "$" /[a-zA-Z_]+/        ; variable
```

### 5.3 Error cases (each points at the offending token)

- Unknown key: `parse error: unknown field 'foo' (try 'help req')`
- Bad u32: `parse error: 'kinds=abc' — expected integer`
- Bad date: `parse error: 'since=tomorrow' — expected YYYY-MM-DD or unix ts`
- Bad tag key: `parse error: '#tags=x' — '#' filters take a single letter`
- Empty value list: `parse error: 'kinds=' — at least one value required`
- Missing required: `req requires at least one of 'kinds' or 'authors' or 'ids'`

---

## 6 — Session state

One struct, owned by the main thread. Mutated only between `req` runs.

```rust
pub struct Session {
    // Identity
    pub seed_hex: Option<String>,        // hex pubkey; None until set-seed

    // Discovery caches
    pub follows_cache: Option<BTreeSet<String>>,                  // kind:3 p-tags
    pub mailbox_cache: BTreeMap<String, MailboxSnapshot>,         // kind:10002

    // Configuration
    pub indexer_relays: Vec<String>,     // default: ["wss://purplepag.es"]
    pub app_relays:     Vec<String>,     // default: []
    pub dead_relays:    BTreeSet<String>,// default: empty
    pub max_connections: usize,          // default: 30
    pub max_per_user:    usize,          // default: 2
    pub wall:            Duration,       // default: 20s

    // Diagnostic state
    pub seen_ids: HashSet<String>,       // delta count for "new events"
    pub last_run: Option<RunSummary>,    // for `show state`
}
```

`MailboxSnapshot` is the planner type re-exported from
`nmp_core::planner::MailboxSnapshot`. `seen_ids` is unbounded for v1 (the
REPL is short-lived; sessions over millions of events are an explicit
non-goal).

---

## 7 — Lifecycle-driven discovery + content pipeline

The REPL drives the production `nmp_core::subs::SubscriptionLifecycle`. It
does **not** hand-roll mailbox discovery, compilation, or selection —
`recompile_and_diff` does all of that internally (implicit kind:10002
discovery REQs, NIP-65 routing, the dead-relay filter, `apply_selection`).
`req` is a tick loop around that one production call.

One `SubscriptionLifecycle` lives per session (not per `req`). Per-session
ownership is the better diagnostic behaviour: the lifecycle's
`probed_mailboxes` set and the `InMemoryMailboxCache` persist across `req`s,
so a second `req` does not re-probe authors already discovered. Both are
`Session` fields (not one wrapper) — `recompile_and_diff` borrows the cache
`&` while `cache.put` needs `&mut`; the mutations never overlap in time so a
split borrow at the call site is the cleanest ownership.

### 7.1 Flow on `req`

1. **Apply session config onto the lifecycle.** `set_indexer_relays`,
   `set_app_relays`, `set_selection_budget(max_connections, max_per_user)`,
   and `mark_relay_dead` for each `dead_relays` entry — so config changes
   between `req`s take effect.
2. **Expand `$follows`** (variable resolution — *not* outbox). If `$follows`
   is referenced and `follows_cache` is empty, do the thin kind:3 fetch
   (`{kinds:[3], authors:[seed], limit:1}`) against the first reachable
   indexer; parse `p` tags; cache. This is exactly what a real
   following-feed ViewModule does to turn `$follows` into a concrete
   `LogicalInterest` author set. `$relays` / `$inbox` read the seed's
   kind:10002 from the lifecycle's mailbox cache.
3. **Build + register the interest.** Construct one `LogicalInterest`
   (`InterestId(1)`, `Global`, `Tailing`) from the parsed filter +
   expanded authors; `lifecycle.registry_mut().push(interest)` (same id
   replaces the prior slot — single-writer registry, D4).
4. **Discovery-convergence tick loop:**
   1. `lifecycle.recompile_and_diff(&mailbox_cache)` → `Vec<WireFrame>`.
   2. Partition by sub_id prefix: `mailbox-probe-*` = implicit kind:10002
      discovery REQs (the lifecycle auto-emits these for any author whose
      mailbox is neither cached nor previously probed); everything else is
      content.
   3. If probe frames exist, print `discovery: probing N mailboxes via
      indexer (K REQs)` and run them synchronously (`fanout::run_discovery`)
      against the indexer. For each kind:10002 response: `parse_kind10002`
      → `cache.put(pubkey, snapshot)` →
      `lifecycle.enqueue_trigger(CompileTrigger::Nip65Arrived { … })`.
   4. `lifecycle.drain_tick(&mailbox_cache)` consumes the `Nip65Arrived`
      triggers; the next iteration's compile routes those authors via
      their declared NIP-65 write relays. `probed_mailboxes` dedups, so an
      author with no kind:10002 is probed exactly once and the loop
      converges in 1–2 iterations.
   5. When a compile yields **no** probe frames, discovery is done.
5. **Materialise the converged content plan.** `recompile_and_diff` returns
   only the *delta* vs. the prior plan, so once the plan stabilises a
   recompile yields little or nothing even though live subscriptions exist.
   The REPL therefore reads the **full** in-effect REQ set via the
   read-only core accessor `SubscriptionLifecycle::current_plan_frames()`
   (one `WireFrame::Req` per `(relay, sub_shape)` in `current_plan`; probe
   REQs are absent by construction — they live outside `current_plan`).
   Unroutable authors come from `current_plan_unroutable()`. Print
   `outbox: N relays, M authors-on-wire, K unroutable`.
6. **Content fanout + live render.** Partition the content REQs per relay
   and hand them to the worker pool (§9). Each worker sends the lifecycle's
   `filter_json` **verbatim** (not rebuilt from the AST). The per-relay
   live table (§8) is unchanged.

### 7.2 Caching rules

- `set-seed` clears `follows_cache` AND replaces the lifecycle + its
  mailbox cache with fresh instances (`reset_lifecycle`) — a new identity
  makes the probed set and cache meaningless.
- `refresh follows` clears `follows_cache` only (variable-expansion state,
  independent of the lifecycle).
- `refresh mailboxes` calls `lifecycle.clear_probed_mailboxes()` and drops
  the mailbox cache so the next `req` re-probes every still-unknown author.
- `refresh` / `refresh all` clears `follows_cache` + does the
  `refresh mailboxes` work.
- `seen_ids` survives `refresh` — it's a session artefact, not discovery.

### 7.3 Indexer / probe socket handling

The kind:3 follows fetch dials the indexer set sequentially and uses the
first reachable URL (v1; multi-indexer fan is a §12 follow-up).

**Finding (lifecycle behaviour, surfaced not papered over):** implicit
kind:10002 probe REQs are appended in `recompile_and_diff` *after*
`auth_gate.partition` and `lifecycle_gate.observe_diff`, and are **not**
inserted into `current_plan`. The lifecycle therefore never tracks them and
never emits a CLOSE for a probe sub. In production the actor lets the
indexer socket drop. The REPL closes each probe sub client-side after EOSE
(`fanout::run_discovery`) so it doesn't leak an open kind:10002 sub. This
is documented in `fanout.rs` and is intrinsic to the production code, not a
REPL-side workaround for a REPL bug.

---

## 8 — Renderer

Lives in `src/render.rs`. Owns the terminal between readlines; rustyline
owns it during readline. No concurrent painting — paint only when invoked
by the main thread between phases or while draining the fanout channel.

### 8.1 Library division

- **`rustyline`** — line editing, history (`~/.cache/nmp-repl/history`),
  tab completion (verbs + variable names).
- **`crossterm`** — `cursor::MoveTo`, `terminal::{Clear, ClearType}`,
  `style::{Color, SetForegroundColor}`. We do not use the event reader,
  raw mode, or alternate screen — those would conflict with rustyline.

### 8.2 Live row table

Before fanout starts, the renderer:

1. Sorts relays by author-count descending.
2. Assigns each relay a stable row index `i`.
3. Prints all rows with state `[connecting…]` and remembers the cursor
   start row `Y0`.

During fanout, the main thread drains the worker `mpsc<RelayEvent>` with
`recv_timeout(50ms)`. On each event:

- `MoveTo(0, Y0 + i)`, `Clear(CurrentLine)`, print the new row.
- After draining a batch, `MoveTo(0, Y0 + N)` to park the cursor below
  the table so the next prompt lands cleanly.

### 8.3 Row states

| State        | Render                                                                                            |
|--------------|---------------------------------------------------------------------------------------------------|
| `Connecting` | `  REQ <url:48>  <authors:>4 authors  [connecting…]`                                              |
| `ReqSent`    | `  REQ <url:48>  <authors:>4 authors  [streaming…]  <events> seen`                                |
| `Receiving`  | `  REQ <url:48>  <authors:>4 authors  <events> events  <new>/<events> new`                        |
| `Eose`       | `> REQ <url:48>  <authors:>4 authors  <events> events  <new> new in <ms>ms`  (green)              |
| `Error(e)`   | `x REQ <url:48>  <authors:>4 authors  <error>`  (red)                                             |
| `Timeout`    | `x REQ <url:48>  <authors:>4 authors  [wall timeout]`  (yellow)                                   |

The leading `>` / `x` glyphs match the user's UX target. URLs longer than
48 chars get truncated with `…`.

### 8.4 ASCII screenshot

```
nmp-repl[seed=npub1l2v…]> req kinds=1 authors=$follows
  phase A: cached (917 follows)
  phase B: 884/917 mailboxes (cached 803, fetched 81 in 412ms)
  outbox: 24 relays, 920 authors-on-wire, 33 unroutable
  REQ wss://relay.damus.io                            83 authors  [streaming…]
  REQ wss://nos.lol                                   78 authors  [streaming…]
  REQ wss://relay.snort.social                        62 authors  [connecting…]
  ...
```

After all rows reach a terminal state:

```
> REQ wss://relay.damus.io                            83 authors  517 events  412 new in 141ms
> REQ wss://nos.lol                                   78 authors  394 events  287 new in 189ms
x REQ wss://relay1.example.com                         3 authors  connect refused
> REQ wss://relay.snort.social                        62 authors  311 events  201 new in 226ms
  fanout: 24 relays, 1812 deliveries, 1493 new (dedup 0.82), wall 1.4s
```

### 8.5 Verbosity flags

- Default: compact (above).
- `-v` (`nmp-repl -v`): per-relay full URL, no truncation; phase-internal
  byte counters; first-EVENT latency.
- `--json` (`nmp-repl --json`): no live table; emit one JSON line per
  state transition on stdout (`{relay, state, events, new, elapsed_ms}`).
  Useful for piping to `jq` / log capture.

---

## 9 — Concurrency model

Verbatim port of `outbox_perf.rs::phase_d_fanout`. Three thread classes:

| Thread        | Count       | Role                                                                                   |
|---------------|-------------|----------------------------------------------------------------------------------------|
| Main          | 1           | Command loop. Owns Session, parser, renderer. Holds rustyline's stdin lock.            |
| Worker        | ≤ 64        | One per relay job. Connects, REQ, drains until EOSE or wall deadline.                  |
| (none)        |             | No background thread between `req` runs. REPL is idle when at the prompt.              |

### 9.1 Channels

```rust
enum RelayEvent {
    Connecting   { relay: String },
    ReqSent      { relay: String },
    Frame        { relay: String, event_id: String, is_new: bool },
    Eose         { relay: String, elapsed: Duration },
    Error        { relay: String, msg: String },
    Done         { relay: String, stats: RelayStats },
}
```

Workers send into a `mpsc::Sender<RelayEvent>`; the main thread holds the
receiver and drives the renderer. The `is_new` flag is computed by the
main thread (workers don't see `seen_ids`) — workers send raw event_id;
main checks `seen_ids.insert(id)` and updates the row counter
accordingly. This keeps `seen_ids` single-writer.

### 9.2 Bounded pool

A `mpsc::channel<(String, Vec<String>)>` work queue + 64 long-lived
worker threads pulling jobs (the `Arc<Mutex<Receiver>>` pattern from
`outbox_perf.rs:451`). The pool exists only for the duration of one
`req`; after the renderer reaches `Done` for every relay (or the wall
deadline expires), worker threads exit and the pool is dropped.

### 9.3 Wall deadline

`Instant::now() + session.wall`. Each worker checks it at the top of its
read loop; the renderer breaks its drain loop on the same deadline. No
mid-frame cancellation — sockets either reach EOSE, error, or get dropped
when their thread returns. Tungstenite read timeout from
`outbox_perf.rs:632` (`READ_POLL = 250ms`) keeps reads cooperative.

### 9.4 No concurrent commands

While `req` is running, the input loop is blocked. This is deliberate —
the REPL is a diagnostic tool, not a multiplexer. A future v2 may grow
named background subscriptions; v1 says "one command at a time".

---

## 10 — Dependencies

All versions match existing workspace pins. Tungstenite, rustls,
serde_json identical to `nmp-core`. Lockfile churn limited to the three
genuinely new crates (`ureq`, `rustyline`, `crossterm`).

```toml
[package]
name = "nmp-repl"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Interactive diagnostic REPL for the NMP planner + outbox."

[[bin]]
name = "nmp-repl"
path = "src/main.rs"

[dependencies]
# Planner + bech32 helpers — the entire reason this crate exists.
nmp-core    = { path = "../nmp-core" }

# Wire stack — pinned identical to nmp-core so we share one rustls.
tungstenite = { version = "0.24", default-features = false, features = ["handshake", "rustls-tls-webpki-roots"] }
rustls      = { version = "0.23", default-features = false, features = ["ring"] }
serde       = { version = "1.0",  features = ["derive"] }
serde_json  = "1.0"

# NIP-05 HTTP resolver — sync, shares the rustls/ring stack via `tls` feature.
# ureq 2.x is API-stable; ureq 3.x exists but has churn we don't need.
ureq        = { version = "2", default-features = false, features = ["tls", "json"] }

# Line editor with history + tab completion. Mature, sync, no async drag-in.
rustyline   = "14"

# Cursor positioning + colour for the live status table. Used only between
# readline() calls — no raw mode, no event reader.
crossterm   = "0.28"

# Date parsing for `since=YYYY-MM-DD`. nmp-core already uses chrono 0.4.
chrono      = { version = "0.4", default-features = false, features = ["clock"] }
```

**Why ureq, not raw rustls + TcpStream:** NIP-05 needs HTTPS GET with
JSON parsing. ureq is ~400 LOC of usage for us, plugs into the same
rustls stack via feature flags, and the alternative is hand-rolling
HTTP/1.1 over rustls — high error surface for no gain. ureq is the
correct tier of "small sync HTTP for one feature".

**Why rustyline, not termion/linefeed:** rustyline has the broadest
platform coverage (Linux, macOS, partial Windows), supports history file
+ tab completion out of the box, and composes cleanly with crossterm
because it only owns the TTY during `readline()`.

**Why crossterm, not raw ANSI escapes:** the live table needs to move
the cursor by row index and clear individual lines. We could write this
in 30 lines of raw escapes, but crossterm gets us cross-platform
correctness for free and the Windows console support we may eventually
want.

---

## 11 — File-by-file implementation order

The order below is "smallest viable thing first, grow outward". An
engineer following this order has a runnable binary after step 3 and a
useful one after step 7.

### Half-day chunks (≤ 4 hours each)

1. **`Cargo.toml` + workspace registration.** Add `crates/nmp-repl` to
   the root `Cargo.toml` `members` list. Empty `src/main.rs` that prints
   "hello" and calls `rustls::crypto::ring::default_provider().install_default()`.
   `cargo build -p nmp-repl` must pass. *(15 min)*

2. **`ast.rs` + `parser.rs` + parser unit tests.** All AST types, the
   tokenizer, and the filter-field parser. No I/O. Tests cover the §5.3
   error cases. *(2–3 hours)*

3. **`session.rs` + `error.rs`.** Plain data; `Session::default()`
   matches the §6 defaults. `ReplError: Display`. *(30 min)*

4. **`nip05.rs`.** Resolve `localpart@domain` via ureq GET to
   `https://<domain>/.well-known/nostr.json?name=<localpart>`; parse
   JSON; return hex pubkey from `names[localpart]`. Unit test with a
   `mockito` server or against `_@f7z.io` as a live smoke test.
   *(1–2 hours)*

5. **`ws.rs`.** Copy `connect`, `try_connect`, `next_text`,
   `normalize_url` from `outbox_perf.rs`. Move them into a module
   so `discovery.rs` and `fanout.rs` share them. *(30 min)*

6. **`commands/set_seed.rs` + REPL loop in `main.rs`.** Wire rustyline,
   parse line → `Command::SetSeed`, dispatch to `set_seed::run`. Handle
   `quit` / `help`. After this step `nmp-repl` is interactive: you can
   type `set-seed _@f7z.io` and it prints the hex pubkey. *(1–2 hours)*

### Day-sized chunks (4–8 hours)

7. **`discovery.rs`.** Phase A + Phase B logic, mirroring
   `outbox_perf.rs::phase_a_fetch_kind3` and `phase_b_fetch_mailboxes`.
   Cache writes into `Session`. Unit tests are integration-style — run
   against a local mock relay (or `wss://purplepag.es` for a smoke test
   committed as `#[ignore]`). *(3–4 hours)*

8. **`plan.rs` + `commands/req.rs` (no renderer yet).** Build the
   `LogicalInterest`, run compiler + selector, print the resulting
   `(url, authors)` map to stdout (no live update — just a static list).
   After this step `req` works end-to-end with batch output.
   *(2–3 hours)*

9. **`fanout.rs`.** Lift `phase_d_fanout` + `run_relay_thread` from
   `outbox_perf.rs` verbatim; emit `RelayEvent` to the channel instead
   of `Msg`. Wire `seen_ids` membership check on the main thread.
   *(2–3 hours)*

10. **`render.rs`.** Live table per §8. This is the most fiddly piece —
    crossterm cursor math, terminal width detection, row truncation.
    Build incrementally: first ANSI-positioned rows that just update on
    every event; then add colour; then the EOSE / Error / Timeout
    glyphs. *(3–4 hours)*

### Polish (half day total)

11. **`commands/show.rs`, `set_app_relays.rs`, `set_indexer.rs`,
    `set_dead.rs`, `set_budget.rs`, `refresh.rs`, `expand.rs`.** All
    trivial — each is ≤ 30 lines. *(2 hours combined)*

12. **`commands/help.rs`.** One static `&str` per verb. *(30 min)*

13. **`-v` and `--json` flags.** Two render modes alongside the default
    compact one. JSON mode skips the live table entirely. *(1–2 hours)*

14. **Tab completion.** rustyline `Completer` trait — complete verbs at
    column 0, variable names after `authors=` / `ids=`. *(1 hour)*

**Total honest estimate: 1.5 days for a solid v1, 2 days with polish.**

---

## 12 — Out of scope (v1)

Each omission with rationale.

- **Writes / publishing.** The REPL is diagnostic. A future `pub`/`event`
  verb is a natural v2 extension; not v1.
- **NIP-42 AUTH.** Relays demanding AUTH for kind:1 fail with a clear
  per-row error. Adding AUTH means key material in the REPL, which is a
  separate threat-model conversation.
- **NIP-77 negentropy.** D2-era concern; outbox correctness is the v1
  target.
- **Multi-seed sessions.** One active seed. Switch via `set-seed`; it
  clears caches.
- **Ctrl+C mid-`req` cancellation.** Wall deadline is the kill switch.
  Doing this right means a `Sigint` handler that races the worker pool;
  doable but not v1-critical.
- **Persistence beyond rustyline history.** Caches are in-memory. NIP-05
  and kind:3 are cheap to re-resolve; persisting them adds invalidation
  questions we don't need yet.
- **Background subscriptions.** No `bg`/`fg` verb. Each `req` is
  start-to-EOSE-or-wall.
- **NIP-19 entities other than npub.** `nevent`, `naddr`, `nprofile` are
  decoded by `nmp_core::nip19::parse` already; wiring them into the
  filter parser is a one-line follow-up but not v1.
- **Bounded `seen_ids`.** Unbounded HashSet is fine for a session that
  runs minutes. A LRU is a v2 nice-to-have.
- **Multi-indexer fan-out for discovery.** v1 uses first indexer only.
  Racing across indexers is a v2 optimisation.

---

## 13 — Implementation pitfalls (call-outs for the engineer)

These are footguns `outbox_perf.rs` already solves; copy its solutions.

1. **rustls provider install.** Call
   `rustls::crypto::ring::default_provider().install_default().expect(...)`
   ONCE at the top of `main()`. Forgetting this makes the first
   tungstenite connect panic deep inside rustls with an unhelpful
   message. See `outbox_perf.rs:58`.

2. **rustyline + crossterm interaction.** Only paint with crossterm
   *between* rustyline `readline()` calls. Concurrent painting during
   input will corrupt the line being edited. The §9.4 "no concurrent
   commands" rule is what makes this safe.

3. **`$follows` before `set-seed`.** Detect at variable-expansion time
   in the executor; return a `ReplError` with the message
   `"$follows requires a seed; run \`set-seed <nip05|npub>\` first"`.
   Never panic.

4. **Mailbox URL normalisation.** kind:10002 events legitimately contain
   URLs with trailing slashes, mixed case, query strings, and the
   occasional `ws://`. Run every URL through the `normalize_url`
   function copied from `outbox_perf.rs:415`. Personal relays
   (`wss://r.x/?broadcast=true`) should NOT be filtered structurally —
   the selector handles them; trust the planner.

5. **Worker pool drop semantics.** When the wall deadline triggers,
   workers may still be in tungstenite reads. Their thread handles must
   either be `join()`ed (slow — up to 250ms each) or detached. v1 detaches
   them; the `MaybeTlsStream` drops cleanly on scope exit and the OS
   reclaims sockets. Document this; don't try to be clever about
   cancellation.

6. **Renderer + scrollback.** Long sessions push history off-screen.
   The renderer must `MoveTo(0, Y0)` where `Y0` is captured *after* the
   table header is printed, not at REPL start. crossterm
   `cursor::position()` returns the post-print cursor row.

7. **Worker count clamping.** `FANOUT_MAX_WORKERS.min(total_jobs.max(1))`
   — exact `outbox_perf.rs:466` line. Don't spawn 64 threads for an
   8-relay plan.

8. **Test the unroutable surface.** A `req authors=$follows` against a
   fresh follow set will always have some unroutable authors (no
   published kind:10002). The "outbox: N relays, M authors-on-wire, K
   unroutable" line is the load-bearing diagnostic — make sure it's
   never zero by accident, never silently dropped, and surfaced
   alongside `show state`.

---

## 14 — Acceptance criteria

v1 ships when:

1. `cargo build -p nmp-repl --release` succeeds with no new warnings.
2. `set-seed _@f7z.io` resolves and prints the hex pubkey.
3. `req kinds=1 authors=$follows` produces the §8.4 live render against
   real relays.
4. `show state` lists current seed, follow count, mailbox count, budget,
   last-run summary.
5. Bad input (parser errors, unknown verbs, missing seed for `$follows`)
   never panics — always a one-line error and a fresh prompt.
6. `--json` mode emits one valid JSON object per relay state transition.
7. A clean exit on `quit` / EOF closes any open sockets in flight.

No automated test gate beyond cargo build + the parser unit tests in
step 2. The whole binary is a manual diagnostic tool; integration-test
against real relays is non-deterministic by design.
