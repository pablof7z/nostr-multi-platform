# kind:0 (profile) timeline gap + relay NOTICE diagnostics — investigation

Status: Investigation only (no code changed). Date: 2026-06-17.
Author: agent investigation per owner request.

Scope: (A) why timeline authors are missing kind:0 / why a name shown last
session shows only the pubkey this session; (B) where relay NOTICE frames live
today and how to surface a per-relay count + a per-relay NOTICE list, reusing
the `relay_diagnostics` projection.

All claims are grounded in `file:line`.

---

## Part A — kind:0 (profile metadata) missing on the timeline

### A.0 How another author's kind:0 reaches the timeline today

1. The profile cache is **in-memory only**. `nmp_nip01::ProfileCache` is an
   `RwLock<HashMap<String, ProfileView>>`
   (`crates/nmp-nip01/src/profile_cache.rs:60-62`), constructed empty
   (`new` → `default`, `:66-69`). Its `cold_cache_is_empty` test
   (`:200-206`) pins that a fresh cache returns `None` for every author. There
   is **no constructor or method that reads stored kind:0 events** — the only
   writer is `upsert_view` (`:80-101`), driven by the `Kind0Parser` ingest
   parser.

2. The **only** trigger that fetches/serves another author's kind:0 is a
   component **claim**. The previous proactive fetch ("fetch kind:0 for every
   author whose kind:1 we ingest") was deliberately **removed** by F-CR-00
   (`crates/nmp-core/src/kernel/proactive_profile_fetch_tests.rs:1-21`:
   "ingesting a kind:1 … does NOT queue a profile fetch … the kernel fetches
   kind:0 ONLY in response to a `claim_profile` from a component"). Chirp issues
   those claims from the timeline row UI:
   `NoteContentView.swift:89`, `NostrAvatar.swift:129`, `NostrProfileName.swift:151`,
   `HomeFeedView.swift:357`, `ProfileView.swift:104` → `KernelModel.claimProfile`
   (`apps/chirp/ios/Chirp/Bridge/KernelModel.swift:440-456`) → `nmp_app_claim_profile`.

3. Kernel side, `claim_profile` lands in
   `crates/nmp-core/src/kernel/requests/profile.rs:113-212`
   (`claim_profile_inner`), which:
   - bumps the refcount in `profile_claims` and `profile_claims_ver`,
   - checks `resident = self.profile_lookup().contains(&pubkey)` (the **in-memory**
     ProfileCache) (`:187`),
   - if resident, runs the TTL re-verify gate (`claim_replaceable`, `:189-192`),
   - computes `want_register = !resident || liveness == Live` (`:200`) and, when
     set, calls `register_profile_claim_interest` (`:209`).

4. `register_profile_claim_interest`
   (`crates/nmp-core/src/kernel/requests/profile.rs:222-280`) builds a
   `LogicalInterest { kinds:[0], authors:[P], limit:None }` and installs it with
   `self.lifecycle.registry_mut().set_sub(identity, interest)` (`:274`), then
   enqueues only a `CompileTrigger::ViewOpened` (`:277-279`). For
   `CacheOk` (feed avatars) the interest is `OneShot` (`:90-95`, `:243-247`).

5. The projection Chirp reads (`claimed_profiles` / `resolved_profiles`) is
   built by iterating `profile_claims` keys and reading the ProfileCache via
   `profile_lookup().profile(pubkey)`
   (`crates/nmp-core/src/kernel/update/projections.rs:377` +
   `crates/nmp-core/src/kernel/update/views.rs:107`). If the cache returns
   `None`, the row renders with the pubkey only.

### A.1 ROOT CAUSE 1 (primary) — the profile-claim interest is NOT store-first (cache-served)

This is the store-first / offline-first bug of ADR-0045 (R3) **applied to other
authors' profiles, where it has not been wired**.

ADR-0045 R3 (`docs/decisions/0045-store-projection-replay.md:145-229`) makes
store-first **universal**: "every interest is served from the local store the
moment it opens … including the active account's own bootstrap kinds (kind:0/3/
10002 …)." The implementation contract is a single seam — every interest install
must route through `enqueue_interest_cache_serve` (the front doors
`push_interest_and_serve` / `ensure_interest_and_serve`, or the deferred +
trailing `run_cache_serve_step` for batch installers)
(`crates/nmp-core/src/kernel/cache_serve/mod.rs:131-242`).

The bootstrap self-kinds path obeys this: `register_oneshot_discovery_interest`
and `register_tailing_self_kinds_interest` both call
`self.enqueue_interest_cache_serve(&sub_key, &shape)` immediately after their
`set_sub` (`crates/nmp-core/src/kernel/requests/startup.rs:195-199` and
`:250-255`). The follow-feed (contacts transition) obeys it too via
`enqueue_interest_cache_serve_deferred`
(`crates/nmp-core/src/kernel/ingest/contacts.rs:155-171`). This is exactly the
contacts store-first fix (ADR-0045) made to work for kind:3 → follow feed.

**The profile-claim path does not.** `register_profile_claim_interest` calls
`set_sub` directly (`requests/profile.rs:274`) and **never calls
`enqueue_interest_cache_serve` / `*_deferred`** — verified by grep: the only
non-test callers of the cache-serve front doors are `actor/dispatch.rs:1326`
(`PushInterest`), `:1346` (`EnsureInterest`), `kernel/mod.rs:2503`
(`open_interest_sub`), `startup.rs:199/255`, and `ingest/contacts.rs:171`.
`requests/profile.rs` is absent.

Consequence: the **only** interest that carries another author's kind:0 is
installed on a path that does not serve the store. So on every cold start the
stored kind:0 events for timeline authors are **never replayed into the
ProfileCache**. The cache starts empty (A.0.1) and the name/picture only appears
*after* a live relay round-trip delivers the kind:0 again (live ingest →
`Kind0Parser` → `upsert_view`). Offline → it never appears. Online → it is
missing until the REQ round-trips (and only if the relay still serves it; see
A.2). This is precisely the reported symptom: "previous session showed the name
(network-fetched into the RAM cache that session), this session shows only the
pubkey (RAM cache empty, store not replayed, waiting on the network)."

Note the cache-serve machinery would already work for this shape if it were
called: `shape_to_store_queries` maps a `{authors:[P], kinds:[0]}` shape to an
`AuthorKind` store query (ADR §3 table; `kernel/cache_serve/queries.rs`), and
the serve dispatches through the `IngestParser` dispatcher
(`PendingCacheServe::needs_ingest_parser_dispatch`,
`kernel/cache_serve/mod.rs:118-128` / `:312-318`), which is where `Kind0Parser`
fires and populates the ProfileCache. The shape is covered; the **enqueue call
is simply missing** at the profile-claim install site.

This is the same class of bug as the contacts store-first issue (ADR-0045): a
replaceable identity kind whose interest was installed without the store-serve
half of the one mechanism. Contacts (kind:3) was fixed at
`ingest/contacts.rs:171`; the active account's own kind:0 was fixed at
`startup.rs:199/255`; **other authors' kind:0 (profile claims) was not** — it is
the remaining R3 gap.

### A.2 ROOT CAUSE 2 (secondary, online case) — the warm-reclaim gate + watermark interaction can suppress re-fetch

Two narrower points compound RC1 once a kind:0 has been seen and stored:

- **Warm-reclaim gate.** `want_register = !resident || Live`
  (`requests/profile.rs:200`). A `CacheOk` claim for an **already-resident**
  profile registers **no** network interest — by design, "the resident store
  serves the card" (`:194-200`). That design *assumes* the resident store
  (ProfileCache) was populated store-first. Because RC1 means it is **not**
  populated on cold start, the gate is moot on first claim (cache empty →
  `resident=false` → it does register and fetch). So online, the first claim
  per author does fire a fetch — RC1 (no store-serve) is the dominant cause of
  the transient/offline gap, not this gate.

- **Watermark is lifecycle-aware and does NOT floor the `CacheOk` fetch.** The
  feed-avatar claim is `OneShot` + `since=None`, which is **exempt** from the
  T129 watermark floor (`crates/nmp-core/src/subs/watermark_rewrite.rs:70-86`:
  non-Tailing + `since=None` stays `None`). So the OneShot kind:0 REQ asks for
  full history and the relay *will* return the stored kind:0 again when online —
  good. The **`Live`** (open profile screen) claim is `Tailing`, which **is**
  floored, but that path also force-fetches, so it resolves. Net: the watermark
  is **not** the cause of the common timeline gap. (It would have been, had the
  feed claim been Tailing — worth keeping in mind if liveness defaults change.)

**Conclusion for Part A.** The dominant root cause is **RC1**: profiles are
never store-first rehydrated because the profile-claim interest install
(`register_profile_claim_interest`, `requests/profile.rs:274`) omits the
`enqueue_interest_cache_serve` call that every other interest install performs.
This is the ADR-0045/R3 store-first ("offline-first") guarantee not being
honored for other authors' kind:0 — the direct analog of the contacts bug. The
fix shape is to route the profile-claim install through the same cache-serve
seam (e.g. call `enqueue_interest_cache_serve(&serve_key, &serve_shape)` after
`set_sub`, mirroring `startup.rs:199/255`), and to serve the store even on the
warm-reclaim branch (`want_register == false`) so a resident-on-disk-but-not-in-
RAM profile is surfaced at open. No code changed here — design only.

---

## Part B — relay NOTICE diagnostics (count in list, full list in detail)

### B.1 Where NOTICEs are received/handled today

NOTICE frames **are already captured** — they are not dropped — but only the
**most recent** one is retained, plus a count.

- Parse + capture hook: `crates/nmp-core/src/kernel/ingest/relay_frame.rs:89-100`
  handles the `"NOTICE"` arm. It truncates the text to 180 chars (`:90-93`),
  then on the live lane (`relay_mut(role)`):
  - bumps the count: `relay.counters.notices_rx += 1` (`:95`),
  - stores last only: `relay.last_notice = Some(notice.clone())` (`:96`),
  - mirrors to the transport map: `record_transport_notice(role, relay_url, notice)` (`:97`),
  - sets `changed_since_emit` and logs (`:98-99`).
- The transport-map mirror: `record_transport_notice`
  (`crates/nmp-core/src/kernel/relay_transport.rs:325-334`) bumps
  `entry.counters.notices_rx` and sets `entry.last_notice` for the per-URL
  `transport_relays` entry.
- Storage: per-relay `RelayHealth` holds `last_notice: Option<String>`
  (`crates/nmp-core/src/kernel/types.rs:380`) and `counters: Counters` whose
  `Counters` struct already has `notices_rx: u64`
  (`crates/nmp-core/src/kernel/types.rs:359-367`). So **the per-relay count
  already exists**; only the last text is retained (no list, no timestamps).
- It is already projected (partially). `relay_diagnostics.rs` carries
  `last_notice: Option<String>` on `RelayDiagnosticsRow`
  (`crates/nmp-core/src/kernel/relay_diagnostics.rs:113-114`, threaded at
  `:336/:348/:362/:383/:431`). The typed sidecar encodes it
  (`crates/nmp-core/src/kernel/tier3_encode.rs:199,215`), and the aggregate
  `Metrics.notices_rx` count is encoded too (`tier3_encode.rs:167`).
- Chirp already renders the single last notice: list row at
  `apps/chirp/ios/Chirp/Features/DiagnosticsView.swift:403` and detail at
  `apps/chirp/ios/Chirp/Features/RelayDetailView.swift:122`, decoded as
  `RelayDiagnosticsRow.lastNotice`
  (`apps/chirp/ios/Chirp/Bridge/KernelSnapshotTypes+RelayDiagnostics.swift:111`).

So this feature is an **extension of an existing, working capture + projection**,
not a new pipeline.

### B.2 Design — per-relay count on the list row + bounded NOTICE list in the detail

Reuse the `relay_diagnostics` projection mechanism (per
`docs/dev/relay-connection-attribution.md` §3.3 / §3.5: NMP captures per-relay
data, pre-formats, exposes via `relay_diagnostics`, Chirp renders). Two new
projection fields; no new projection key, no new FFI symbol.

**Kernel retention (bounded, D0/D8).**

1. Replace the single `last_notice: Option<String>` on `RelayHealth`
   (`types.rs:380`) and on the transport entry with a bounded ring:
   add `notices: VecDeque<NoticeEntry>` where
   `struct NoticeEntry { at_ms: u64, text: String }`. Keep `last_notice`
   derivable as `notices.back()` (or retain it for back-compat; either way one
   source of truth). Bound the ring with a per-relay cap constant, e.g.
   `RELAY_NOTICE_LOG_CAP = 32`, oldest-dropped — mirroring the
   `RoutingTraceProjection` bounded-`VecDeque` precedent (default cap 64,
   `crates/nmp-core/src/kernel/routing_trace.rs:44`). This keeps memory O(relays
   × 32 × 180 bytes), satisfying D0/bounded-memory.

2. Capture hook (the single place to push): `relay_frame.rs:95-97` and the
   mirror `relay_transport.rs:325-334`. Push `NoteEntry { at_ms: now, text:
   truncate(s, 180) }`, pop_front when over cap, and still bump
   `counters.notices_rx` (the **lifetime** count, which can exceed the retained
   list length — that is the value to show as "count"). Timestamp source: the
   same `now_ms` the projection already uses for `last_connected_ms` /
   `last_event_ms` raw-epoch fields (`relay_diagnostics.rs:60-62` notes raw
   epoch ms are carried; shells format "Xs ago").

**Projection (`relay_diagnostics.rs`).**

3. List row: add `pub(super) notice_count: u64` to `RelayDiagnosticsRow`
   (alongside `last_notice`, `relay_diagnostics.rs:113-114`). Populate from
   `counters.notices_rx`. This is the "how many NOTICEs has this relay sent us"
   shown in the relay LIST. (Optionally a pre-formatted `notice_count_display`
   compact string, following the `total_events_display` precedent at
   `:97-99`, so the shell never formats.)

4. Detail: add `pub(super) notices: Vec<RelayDiagnosticsNotice>` where
   `struct RelayDiagnosticsNotice { at_ms: u64, text: String }` (newest-last or
   newest-first, decided in Rust). Populate from the bounded `notices` ring.
   This is the full retained list shown in the relay DETAIL.

**Codegen chain (ADR-0037 — full chain, same as the attribution feature §3.3.5).**
Touch all of: `crates/nmp-core/schema/relay_diagnostics.fbs` (add
`notice_count:ulong` on the row + a `RelayDiagnosticsNotice` table +
`notices:[RelayDiagnosticsNotice]`); regenerate the Rust reader/writer and the
Swift reader
(`crates/nmp-core/src/kernel/typed_projections/generated/relay_diagnostics_generated.rs`,
`apps/chirp/ios/Chirp/Bridge/Generated/…`); update the Rust typed model + encoder
(`tier3_encode.rs` near the `last_notice` encode at `:199/:215`) + decoder; the
Swift DTOs (`KernelSnapshotTypes+RelayDiagnostics.swift:90-139`); and the glue
(`TypedProjectionGlue+RelayDiagnostics.swift:30`). Regenerate via
`crates/nmp-codegen`. Add an ADR-0037 JSON↔FlatBuffer parity test for a row
carrying `notice_count` + `notices`. No new projection key, so
`builtin_projection_keys_const_matches_runtime` is unaffected; the generated-code
drift gate IS affected and must pass.

**Chirp render (thin shell).**

5. List: `DiagnosticsView.swift` (near the existing `row.lastNotice` use at
   `:403`) shows `row.noticeCount` (e.g. a small badge "N notices").
6. Detail: `RelayDetailView.swift` (near `:122`) replaces/augments the single
   `row.lastNotice` block with a section iterating `row.notices`, each rendering
   `text` + a "Xs ago" formatted `atMs` (reuse the existing relative-time
   formatter the view already uses for connect/event timestamps).

**D0/bounded-memory note.** The retained per-relay NOTICE log is capped
(`RELAY_NOTICE_LOG_CAP`, oldest-dropped), the count is a saturating `u64`
lifetime counter (already present), and the text is truncated to 180 chars at
capture (`relay_frame.rs:93`). No unbounded growth; the cap is the only new
memory knob and matches the routing-trace ring precedent.

### B.3 Smallest viable version

`notice_count` alone (step 3 + its codegen + the list badge) is a one-field
extension that reuses the **already-incremented** `counters.notices_rx` — it
requires no retention change at all. The bounded `notices` list (steps 1-2, 4)
is the larger half (changes `RelayHealth` storage + adds a table to the schema)
but is still entirely within the existing capture hook and projection.

---

## Summary of findings

- **Part A root cause:** other authors' kind:0 is fetched **only** on component
  claim (proactive fetch removed, F-CR-00), and the claim-install path
  `register_profile_claim_interest` (`requests/profile.rs:274`) installs the
  kind:0 interest via `set_sub` **without** the `enqueue_interest_cache_serve`
  call every other interest install uses. The `ProfileCache` is in-memory only
  (`profile_cache.rs:60-69`) and is never rehydrated from the store. So stored
  kind:0 is not replayed on cold start → name/picture missing until a live relay
  round-trip (offline: forever). This is the ADR-0045/R3 store-first guarantee
  not honored for profiles — the exact analog of the contacts (kind:3) bug,
  which WAS fixed (`ingest/contacts.rs:171`) as was the self account's kind:0
  (`startup.rs:199/255`). The watermark is lifecycle-aware and does NOT suppress
  the OneShot feed-avatar fetch (`watermark_rewrite.rs:70-86`), so it is not the
  cause. Fix shape: route the profile-claim install through the cache-serve seam
  and serve the store on the warm-reclaim branch too.

- **Part B:** NOTICEs are already captured (`relay_frame.rs:89-100`), counted
  (`Counters.notices_rx`, `types.rs:363`), last-only retained
  (`RelayHealth.last_notice`, `types.rs:380`), projected
  (`relay_diagnostics.rs:113`) and rendered (`RelayDetailView.swift:122`,
  `DiagnosticsView.swift:403`). Design: surface `notice_count` (from the
  existing `notices_rx`) on the list row, and replace `last_notice` with a
  bounded per-relay `VecDeque<NoticeEntry{at_ms,text}>` (cap ~32, oldest-dropped,
  routing-trace precedent) projected as `notices:[…]` on the detail — both
  through the full `relay_diagnostics` FlatBuffers codegen chain. Capture hook is
  the existing `relay_frame.rs:95-97` + `relay_transport.rs:325-334`.
