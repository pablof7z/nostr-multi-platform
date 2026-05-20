# Opus Direction Review — 2026-05-20 09:03

Principal-architect review. Skeptical, not a checkpoint. Where this overlaps the
earlier 2026-05-20 review (`project_direction_opus_review`), it cites new evidence
or disagrees outright. The verdict here is different: the riskiest bet is **not**
the relay transport — it is the per-app projection pattern.

## 1. What NMP does NOT support that it clearly should

**The `actions` layer from aim.md §4.3 does not exist.** aim.md promises "all
writes through actions" — composable, atomic publish-plus-local-state. The FFI
delivers the opposite: 50+ hand-rolled C symbols, one per verb —
`nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`,
`nmp_app_add_relay`, `nmp_app_remove_relay`. There is no `ActionContext`, no
action runner, no sub-action composition. Every new write verb is a new C symbol
on every platform. This is the single largest gap between the stated design and
the code.

**No reactive event store with claim-based GC (aim.md §4.1).** There is
`MemEventStore`, `LmdbEventStore`, and a `profile_claims: HashMap<String,
BTreeSet<String>>` field on `Kernel` (capped at 256). That cap is a manual
per-view leak patch, not a general claim system. `prune()` does not exist —
`grep 'fn prune'` returns nothing. `events: HashMap` is documented insert-only.
Memory grows without bound; aim.md §4.1's central promise is unmet.

**Protocol gaps that block real apps:** NIP-17 conversation layer (gift-wrap
primitives exist in `nmp-nip59`; the DM/conversation layer does not — `send a
DM` is impossible today). Blossom (no crate, no code). NIP-65 read is wired but
there is no NIP-51 list management surface. NIP-44 is buried in marmot, not a
first-class kernel capability. Background-decryption NSE surface (aim.md §4.10,
§7.5) is undesigned.

## 2. What NMP should STOP doing or simplify

**Stop shipping per-app projection crates as the integration pattern.** Chirp
ships `ChirpModularTimeline` registered as a kernel event observer, emitting its
own JSON via `nmp_app_chirp_snapshot`. Marmot does the identical thing. Each app
now needs: a protocol crate + a projection crate + 4-6 bespoke FFI symbols + a
JSON payload type + a Swift decoder. The bible anti-pattern "business logic in
ViewState derivation" was not eliminated — it was relocated from Swift into
per-app Rust and **duplicated per app**. aim.md §4.2 promised reusable typed
views; the code builds a new one each time.

**Stop calling PR #19 a decomposition.** It claimed "15 view-tracking fields → 3
sub-structs." The `Kernel` struct still inlines **76 fields** (verified count).
Sub-modules were split out; the god-struct was not. `selected_author`,
`selected_thread`, `diagnostic_firehose`, `pending_thread_ids`,
`requested_thread_ids`, `thread_ids_inflight`, `wire_subs`, `oneshot_subs`,
`auth_signers`, `publish_queue`, `event_provenance` — all still direct fields.
The bible's >1000-LOC rule was satisfied cosmetically.

**Remove `wallet_status` from the `Kernel` struct.** It is `#[cfg(feature =
"wallet")] wallet_status: Option<WalletStatus>` — a feature-gated app noun
*inside the substrate*. D0 says no app nouns in the kernel. A feature flag does
not make it doctrine-clean; it makes the violation conditional. Wallet state
belongs in a `nmp-nwc` projection registered through the observer seam, exactly
like marmot.

**Half-finished, decide now:** the `lifecycle`/`InterestRegistry`/`LogicalInterest`
machinery coexists with the M1 hand-rolled `req()` path — two subscription
systems, the new one "dormant" (kernel comment, line 289-298). PR #21's
`into_logical_interest` bridge is built but the migration that consumes it is
not. Finish the migration or delete the dormant path; do not ship both.

## 3. The single riskiest architectural bet

**The per-app projection + bespoke-FFI pattern — not the relay transport.**

The earlier review named the hand-rolled relay transport. Disagree. ADR-0022
covers that, it has tests, and a transport rewrite is contained — it is one
crate behind a clear seam. The projection pattern is riskier because it is
**load-bearing for the entire framework thesis and a UniFFI migration will not
fix it.**

Evidence it is wrong: the success criterion in aim.md §1 is "one-shot a working
Nostr app in a few hundred lines of UI code." But adding an app today means
writing a Rust projection crate, a payload type, 4-6 `#[no_mangle]` symbols, and
a Swift decoder — *per app*. Chirp and Marmot already prove the pattern
duplicates rather than composes. If this is how app #3 gets built, NMP is a
substrate library that each app re-skins, not a framework. The FFI surface grows
linearly with verbs × apps; M14 (UniFFI) re-encodes the same explosion in a
different binding generator — it does not collapse it.

## 4. Next concrete 1-week deliverable

**Build a second non-social app end-to-end and measure its boilerplate.** Take
`nmp-highlighter` or `nmp-podcast` to a running iOS screen using *only generic
kernel projections and a generic `dispatch_action(name, json)` entry point* —
no new projection crate, no new bespoke FFI symbols.

This is the sharpest possible discriminator and it tests the whole thesis in one
shot:

- If app #2 needs its own projection crate + FFI surface (like Chirp/Marmot),
  the projection pattern is confirmed broken — NMP is a substrate, not a
  framework, and the abstraction must be redesigned before M14.
- If app #2 ships in <100 LoC Rust + <300 LoC Swift over generic projections,
  the thesis holds and the FFI explosion has a proven fix worth generalizing.

Pair it with one concrete refactor as the enabling step: collapse `react`,
`follow`, `publish_note` into a single `nmp_app_dispatch_action(name, json)`
backed by an action registry. That simultaneously builds the missing aim.md §4.3
actions layer and kills the per-verb FFI growth. If that refactor cannot be done
cleanly in a week, that itself is the most important finding of the week.

Defer the load test and UniFFI. They prove things sequentially; the second-app
build proves or breaks the core bet immediately.
