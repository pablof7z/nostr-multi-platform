# ADR-0045 — Revisions 2 & 3

This file contains the owner corrections to ADR-0045.
The original ADR text is in [0045-store-projection-replay.md](0045-store-projection-replay.md).

## Revision 2 (2026-06-12) — owner correction: one mechanism, always on

This revision supersedes **§9 (staged rollout)** and **§11 (v1 recommendation)**
and amends the **Decision (§2)**. The remaining sections (§1, §1.1, §1.2, §3–§8,
§10) are Rev 1 text and stand as written — their *technical* findings survive the
correction intact (enumerated below). Where Rev 1 says "stage 1" / "stage 2" /
"stage 3" as a **product-staged-by-domain** boundary, read it as Rev 2's single
mechanism with *engineering* shape-coverage increments — never as a per-domain
gating of when cache-serve "turns on."

### R2.1 The correction (verbatim, owner, 2026-06-12)

> "offline replay should be governed by a single mechanism which is 'get events
> from the network' — per how we implement things in NMP there should be a
> SINGLE way to do things, and that thing should include serving things from the
> cache. So the 'stage 1 (timeline)' vs '(stage 2) DMs' should not even exist; it
> should be the same thing and serving things from the cache should always happen
> regardless of whether the app is just starting and it's not even connected to
> relays or if the app is already connected to relays."

### R2.2 What this changes

The staged-by-domain framing in §9 (timeline first, DMs second, generalize third)
is **rejected as design.** There is exactly **one** event-acquisition mechanism,
and serving from the local store is the **first half of that one mechanism** — not
a separate offline mode, not a per-domain feature that lands timeline-then-DM:

- **One seam, every interest.** Every `LogicalInterest` — any shape, any consumer,
  host-declared or built-in, timeline or DM or thread or long-form or anything
  else — when opened / compiled, is **served from the local store first** through
  the *same* post-store projection-dispatch path that relay-delivered events take
  (§1.2: `insert_timeline_id_sorted` + `events` read-cache + `notify_event_observers`,
  never `store.insert`). The planner's wire REQ is the **refinement half of the
  very same mechanism** — it widens coverage and pulls events the store does not
  yet hold.
- **Always on; no special cases.** Cold start, warm app, offline, online — the
  store-serve half runs unconditionally for every opened interest. There is no
  "offline path" and no "online path"; there is one path with two halves (local
  store → wire). **Offline rendering is simply the degenerate case where the
  network half delivers nothing** — the store half already rendered the view.
- **The watermark⇄replay invariant (§6) now holds universally by construction.**
  Because cache-serve is part of the one mechanism that every floored shape passes
  through, "no watermark floor without cache-serve for the same shape" is true for
  *all* shapes automatically. The §6/§9-stage-3 CI assertion therefore becomes a
  **structural guard** (the seam is the same seam) rather than a per-shape coverage
  checklist that DMs/threads/long-form each have to earn.

### R2.3 Alignment with the north star (this was drift)

`docs/aim.md` §4.1 ("Reactive single source of truth") already promised exactly
this and the staged design was drift from it:

- §4.1: *"Every read goes through it [the EventStore]."* — i.e. a projection open
  is a read, and reads are served from the store first. A staged design where the
  feed reads through the store on second launch but the DM inbox does not (until a
  later stage) violates "every read goes through it."
- §4.1's **fallback event loader**: *"a single user-provided async function the
  store calls when a subscription asks for an event it doesn't have… the developer
  never writes 'if missing, fetch from relay, then update local state' logic."*
  That is precisely the two-halves-of-one-mechanism shape: store-serve first, wire
  refinement on miss, **one** path. The wire REQ in this ADR is NMP's in-kernel
  realization of that fallback half; the store-serve seam is the read half.

The single-mechanism design is therefore a **return** to aim.md §4.1, not a new
position.

### R2.4 What survives from Revision 1 (technical findings — unchanged)

All of Rev 1's load-bearing engineering findings survive the correction; they are
*how* the one mechanism is built correctly:

- **(a) No `store.insert` on the serve half.** A re-fed stored event returns
  `InsertOutcome::Duplicate` and the `Duplicate` arm is a deliberate no-op
  (§1.2, §8(c)) — cache-serve must feed the **post-store projection-dispatch**
  seam directly, the inverse of the live insert-then-project path. (Universal now:
  it is the same seam relay deliveries land on after insert.)
- **(b) `Provenance::LocalStore` marker** distinguishes store-served events from
  wire-delivered ones in the one dispatch path (§2, §10).
- **(c) Budgeted per-tick serve on the actor thread** — at most
  `REPLAY_BUDGET_EVENTS` per interest per tick, chunked continuation, never an
  unbudgeted whole-store scan (§5, the #1085 / V-117 anti-precedent) and never a
  blocking scan at construction (#617).
- **(d) `InterestShape` → `StoreQuery` mapping** over existing indexes (§3), no
  new index required for the shapes the planner compiles today.
- **(e) The watermark ⇄ serve invariant** (§6) — *"no watermark floor without
  cache-serve for the same shape"* — now holds **universally by construction**
  because cache-serve is part of the one mechanism every floored shape uses,
  making the CI lint a structural seam-identity check rather than a per-shape
  table.
- **(f) DM ciphertext** (NIP-17 kind:1059) is store-served through the **#1080
  decrypt seam**, as a *property of the uniform path* (the same seam unwraps live
  gift-wraps) — **not** a separate "stage 2." Verify the decrypt seam does not
  assume live-only provenance; that verification is engineering work on the one
  mechanism, not a product stage.
- **(g) MLS group state stays excluded** (§7): MLS ratchet/group state is a
  stateful protocol object rehydrated by Marmot's own persistence, **not**
  event-acquisition, so it is outside this mechanism by definition (replaying
  kind:44x into MLS would corrupt ratchet state). Pure event-projection group
  *membership/metadata* shapes ride the one mechanism like any other shape.

### R2.5 Amended Decision (§2)

§2 is amended to state the contract as a **single always-on cache-serve seam**:
the store-serve half of the one event-acquisition mechanism runs for **every**
opened/compiled `LogicalInterest`, unconditionally (cold/warm/offline/online),
feeding the post-store projection-dispatch path with `Provenance::LocalStore`,
budgeted on the actor tick; the planner's wire REQ is the refinement half of the
*same* mechanism. The store→`StoreQuery` mapping (§3), ordering/limits (§4),
budget (§5), invariant (§6), DM decrypt routing (§7), and the rejected
alternatives (§8) are unchanged and apply to this single seam.

### R2.6 Engineering may still land incrementally — but the contract is one seam

Implementation **may** land in increments (e.g. broadening shape coverage,
budget tuning), but those are *engineering increments of one mechanism*, not
product stages and not a per-domain gating of when cache-serve exists. The design
contract is the single always-on seam, and the **acceptance test is universal**:

> **Launch twice; the second launch offline. EVERY open interest's projection
> renders from the store** — feed, DM inbox, threads, long-form, and anything
> else the app has opened. Offline-empty for any open, store-backed interest is a
> failure of the one mechanism.

See §11 (revised) for the v1 recommendation under this shape.

---

## Revision 3 (2026-06-17) — owner correction: store-first is UNIVERSAL

Revision 2 established "one mechanism, always on." Revision 3 states the rule at
its full reach: **every interest is served from the local store the moment it
opens, and revalidated by its wire REQ — including the active account's own
bootstrap kinds (kind:0 profile, kind:3 contacts, kind:10002 relay list,
kind:10000/10006).** The store-serve half and the wire REQ are the two halves of
the one mechanism (R2.2), and they run together for **every** interest the system
compiles — host-declared or built-in, consumer or bootstrap, discovery-direction
or follow-feed. Store-first is the universal default; the network is the
revalidation layer on top of it.

### R3.1 The principle (owner, 2026-06-17)

> "Store-first applies to everything by default. We serve the kinds we need from
> the cache; later, if we find a kind:3 — or any kind — with fresher data, that is
> the EXACT SAME THING as a new version of an event being signed right now. It
> should LITERALLY be the same code.
>
> Scenario: the app opens. We serve the kinds we need from the cache, find a
> kind:3 signed at t+0, show it to the app, query the relay, find the exact same
> event — nothing happens, we already have the latest version. Later, a new
> version is signed in some other client; we see the event come in; we route it
> through the same mechanism, which results — without the app doing ANYTHING — in a
> resubscription of the kinds shown in the timeline under the new kind:3. The app
> doesn't even know some other client followed someone new; it just keeps
> receiving the data it needs to show.
>
> Store-first is what makes every NMP app perfectly offline-first."

### R3.2 The two halves run together, for every interest

Cache-serve is the **first half** of the one mechanism; the wire REQ is the
**refinement half**. They are **additive** — both always run:

- **Serve from cache, then revalidate.** On open, every interest is served from
  the local store immediately, and its wire REQ (tailing or one-shot) fires in
  parallel to refine and tail for future updates. Time-to-first-pixel is **zero**;
  the network keeps the view current from there.
- **One code path for cached and fresh — literally the same code.** A
  store-served event and a relay-delivered event flow through the *same*
  `project_accepted_event` seam under the *same* supersession rule (newest
  `created_at` wins; event-id tiebreak). When the relay returns the event we
  already hold, supersession makes it a no-op; when it returns a newer one — or
  another client signs one mid-session — the same seam re-drives every downstream
  effect (the contacts transition re-registers the follow-feed; the timeline
  re-subscribes under the new follow set). Cold-start serve and live arrival
  differ ONLY by `Provenance` — never by mechanism.
- **The on-disk copy is authoritative offline.** When the app is offline, or in
  the window before the REQ round-trips, the last-known copy on disk **is** the
  current copy — and rendering it immediately is exactly right. This is what makes
  the app show your follow list, your profile, and your relay list on a plane, in
  line with aim.md §4.1 ("every read goes through the store").

### R3.3 Store-first resolves the cold-start chicken-and-egg

The active account's NIP-65 mailbox (kind:10002) and follow set (kind:3) are
needed to drive everything downstream — and store-first delivers them **first**:

- Serving the stored kind:10002 yields the last-known relay list **immediately**,
  so the first wire REQs target the user's real outbox. (The indexer-relay
  fallback remains for the cold-cold case: a brand-new install with no kind:10002
  on disk yet.)
- Serving the stored kind:3 populates the contacts cache at startup, firing the
  contacts transition (`on_active_contacts_changed`) → registering the follow-feed
  → serving the followed authors' notes. The entire timeline rehydrates from disk
  with the app doing nothing — the offline-first acceptance test (R2.6) passes for
  the bootstrap kinds, not just consumer feeds.

### R3.4 What the implementation does

- **Every built-in interest cache-serves on open.** The bootstrap self-kinds
  tailing interest and the one-shot discovery interests
  (`kernel/requests/startup.rs`) route through `enqueue_interest_cache_serve` like
  every other interest, serving kind:0/3/10002/… from the store on open. Their
  tailing / one-shot wire REQ fires alongside for revalidation (R3.2).
- **The §6 watermark⇄serve invariant binds the bootstrap shapes.** kind:0/3/10002/…
  are watermark-floored, therefore they are cache-served — the invariant ("no
  watermark floor without cache-serve for the same shape") applies to them as to
  every other floored shape.
- **Every code comment frames cache-serve as the default for all interests** —
  the store-serve half runs for each interest, with the wire REQ as the refinement
  half on top.

Store-first is the law for every interest the system opens.

---
