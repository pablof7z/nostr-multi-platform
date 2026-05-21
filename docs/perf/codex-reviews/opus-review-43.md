# Opus Direction Review #43 — 2026-05-21

## What just shipped

Two things landed since #42. First, a **D0 doctrine sweep** (`cd5416ae`,
`39bff1e8`, `db76fb4d`, `17e81b20`, `e22d63fe`) purged ~25 app-layer nouns
("Marmot"/"Chirp"/"iOS"/"Android"/"SwiftUI") from doc comments across
nmp-nip29, nmp-nip59, nmp-nwc, and nmp-core — plus a real correctness fix
(`stable_hash64` replacing `DefaultHasher` for the giftwrap `InterestId`).
Second, PR #137 (`3f2bd3f7`) added `Kernel::recipient_dm_relays()` — the
kind:10050 DM-relay lookup **seam** — and pointed `dm.rs` at it. **But the
seam returns `None` unconditionally, and the `None` fallback is the
sender's Content relays.** PR #137 looks like it closed #42 Risk #1. It did
not. It converted a TODO into a silently-wrong path that logs a warning.

## Inert surface inventory

### Class C — *new* — the inert-seam-with-warn

This review names a third inert pattern, distinct from #42's two classes.
Class A does nothing. Class B silently produces empty output for a user
class. **Class C does the *wrong* thing, logs a `warn!` about it, and
passes CI** — the worst of the three, because the log line reads as
"handled."

- **`Kernel::recipient_dm_relays` — seam returns `None` forever, fallback
  is wrong.** `crates/nmp-core/src/kernel/outbox.rs:172-176`: the body is
  `None`, with no kind:10050 cache to read. The consumer
  `send_gift_wrapped_dm` (`crates/nmp-core/src/actor/commands/dm.rs:154-162`)
  treats `None` as "fall back to `kernel.bootstrap_urls_for_role(Content)`"
  — the **sender's** Content relays — for **both** envelopes (recipient and
  self-copy), and emits a `tracing::warn!`. Review #42 Risk #1 specified the
  fix precisely: recipient envelope → recipient's kind:10050; self-copy →
  sender's kind:10050; fallback chain → recipient's kind:10002 **read**
  relays, then discovery seed. PR #137 implemented none of that routing —
  only the seam shell and the warn. The DM-delivery bug is **structurally
  unchanged since #42**. The send still routes to the wrong relays; the only
  delta is a log line.

### Class B — silently inert for a whole user class (unchanged since #42)

- **`DmInboxProjection` returns `{conversations: []}` forever for bunker
  (NIP-46) users.** `crates/nmp-nip17/src/inbox.rs:231-241`: `ingest_gift_wrap`
  resolves the active account's keys from the `local_keys` slot; `None`
  (remote signer) is a silent no-op. `snapshot()` then emits
  `{conversations: []}` (`inbox.rs:206,216`). A bunker user signs in, opens
  the DM screen, sees an empty inbox — no toast, no placeholder, no log.
  #42 asked for a 1-line `{"unsupported": "..."}` placeholder field. **Not
  done.** Still silent.

### Class A — fully inert (unchanged since #42)

- **ADR-0026 NIP-44 seam — still zero non-test callers, now on the 1-cycle
  clock.** `RemoteSignerHandle::nip44_encrypt` / `nip44_decrypt`
  (`crates/nmp-core/src/remote_signer.rs`) have only the trait def, the
  `ArcRemoteSigner` forwarders, the `Nip46Signer` impl, and unit tests.
  ADR-0026 scheduled the `dm.rs` migration onto this seam as "Phase 3."
  Phase 3 shipped (PR #136) without it; PR #137 did not touch it either.
  #42 put it on a hard one-cycle clock. **That cycle is now elapsed.** Next
  review: consume it or cut it.

- **`DomainModule` migration machinery** (`substrate/domain.rs`,
  `EventStore::run_migrations` at `store/events.rs:375`) — zero production
  driver, test-only callers. Unchanged. Not urgent; do not cite as
  "infrastructure ready."

## Top 3 risks

### 1. kind:10050 mis-routing is now *camouflaged by a warn* (CORRECTNESS)

`dm.rs:154-162` routes both gift-wrap envelopes to the sender's Content
relays. NIP-17 § 2 requires each envelope to go to the **receiver's**
kind:10050 list. What breaks, how, when:

- User taps send → `nmp.dm.send` dispatches → actor publishes to the
  sender's own relays → **no toast** (the `warn!` does not surface to the
  user, by design — see `dm.rs:318-347` which pins exactly this). The send
  "succeeds."
- If the recipient reads a different relay set (the normal case — that is
  the entire reason kind:10050 exists), the DM is **never delivered**.
  Silent data loss on the primary messaging path.
- The `tracing::warn!` makes this *worse* than the pre-#137 TODO: a reviewer
  scanning for "is DM routing handled?" finds a seam, a fallback, and a log
  line, and concludes it is handled. It is not. The warn is the camouflage.
- Secondary: publishing the kind:1059 envelope to the sender's known relay
  fingerprint partially defeats NIP-59 outer-key unlinkability.

This is the same bug #42 flagged as Risk #1. One cycle later it is still
live, and the seam shipped in between makes it *look* addressed.

### 2. Bunker users get an empty DM inbox forever (WHOLE-CLASS FAILURE)

Unchanged from #42 Risk #2. `inbox.rs:231-241` requires a local-key slot;
remote-signer accounts never populate it. `nip17.dm_inbox` is
`{conversations: []}` permanently for every bunker user, with nothing in the
snapshot to explain why. CI is green — every inbox test uses
`Keys::generate()`. Invisible until a real bunker user reports "my DMs are
empty." The send side at least toasts (`dm.rs:82-89`); the receive side is
silent. The asymmetry should be closed in one cycle.

### 3. Chirp LOC is *growing* on a broken foundation (DISCIPLINE)

Chirp Swift is now **10,743 LOC** (`ios/Chirp/Chirp/*.swift`) — up from
9,944 at #42, ~36× the 300-LOC thin-shell budget. The +799 is largely the
Phase 3 DM screens: `DmListView.swift` (229), `DmConversationView.swift`
(156), `DmBridge.swift` (178). The individual DM files are genuinely thin —
`DmBridge.swift` is verified protocol-logic-free, it only mints C-string
args and forwards. The risk is not creep in those files; it is that the
project shipped a whole new UI surface (and grew the shell 8%) on top of a
DM delivery path that does not deliver. The #42 recommendation to **freeze
new Phase N NIP-17 UI until kind:10050 routing works** was not enforced.

## What to cut or freeze

- **RE-AFFIRM the freeze, and acknowledge it was not honored.** #42 said:
  freeze any new "Phase N" NIP-17 PR that does not fix kind:10050 routing or
  consume the ADR-0026 seam. Phase 3's DM *screens* shipped anyway. The next
  NIP-17 PR MUST be the kind:10050 ingest path (below) — nothing else.
- **ADR-0026 seam: the 1-cycle clock is up.** Either the next NIP-17 PR
  consumes `nip44_encrypt`/`nip44_decrypt` (it naturally would, if it also
  does the bunker-DM seal), or delete both methods, the `ArcRemoteSigner`
  forwarders, and the `Nip46Signer` impl. Do not let it ride a third cycle.
- **Do not ship another seam for kind:10050.** PR #137 already shipped the
  seam. The next PR must fill it with a real cache + ingest path, not add
  another layer of indirection. If a PR titled "kind:10050" lands and
  `recipient_dm_relays` still returns `None`, reject it.
- **`DomainModule` migration machinery** — still a cut candidate; not
  urgent. Do not cite it as ready infrastructure.

## What to build next

**Single highest-ROI PR: the kind:10050 ingest path — the real cache, not
another seam.** `crates/nmp-core/src/kernel/ingest/relay_list.rs` is 98
lines and is a verbatim template. Concrete changes:

1. **`crates/nmp-core/src/kernel/ingest/dm_relay_list.rs`** (new) — clone
   `relay_list.rs`. kind:10050 is replaceable like kind:10002. Two
   differences from the kind:10002 parser: (a) the tag name is `relay`, not
   the `r` marker NIP-65 uses; (b) kind:10050 has a single relay bucket (no
   read/write/both split) — a flat `Vec<String>`. Keep the same
   empty-list-removes-entry and strict-`>` supersession guards.

2. **Add `dm_relay_lists: HashMap<String, DmRelayList>` to `Kernel`**
   (`kernel/types.rs` for the `DmRelayList` struct, the `Kernel` struct for
   the field). `DmRelayList { event_id, created_at, relays: Vec<String> }`.

3. **Add a `10050` arm to `kernel/ingest/mod.rs:376`** match — identical
   shape to the `10002` arm: `verify_and_persist`, gate on
   `Inserted | Replaced`, call `ingest_dm_relay_list(event)`.

4. **Replace `recipient_dm_relays`'s body** (`outbox.rs:172-176`): look
   `pubkey` up in `dm_relay_lists`; return `Some(relays)` on hit (including
   `Some(vec![])` for the documented "published 10050 but empty" case). On
   miss, return `None` — and **fix the `dm.rs` fallback** (`dm.rs:154-162`)
   to chain `recipient_read_relays(receiver_hex)` (the kind:10002 *read*
   role — the recipient's NIP-65 inbox, semantically the right fallback),
   then the discovery seed. The current `bootstrap_urls_for_role(Content)`
   call is wrong on both the "whose relays" and "which role" axes.

5. **Push a kind:10050 fetch interest** when a DM conversation opens (or a
   recipient pubkey is first seen) — mirror the existing kind:10002 gossip
   path so the resolver is not perpetually on the fallback branch.

**Bundle the bunker placeholder (Risk #2) in the same PR — it is one line.**
In `inbox.rs::snapshot()` (`inbox.rs:206`), detect "signed in but no local
keys" (the bunker case) and emit
`{"conversations": [], "unsupported": "bunker accounts cannot yet decrypt
DMs"}`. The Swift `DmListView` already renders `conversations`; an optional
`unsupported` field is purely additive (D1: a placeholder is part of the
type contract). This converts a silent whole-class failure into an honest
screen for the cost of one `if`.

## Structural health snapshot

- **D0 structural violations: none.** Grep confirms zero app-crate imports
  (`nmp_chirp`, `nmp_marmot`, `nmp_app_chirp`) in nmp-nip17, nmp-nip29,
  nmp-nip59, nmp-nwc, or nmp-core. The 25 doc-noun removals were
  cosmetic-only — correct housekeeping, no structural change. The D0 sweep
  is genuinely complete at the structural level.

- **ADR-0025 exceptions: `marmot_local_nsec` is the only structural one**
  (`actor/mod.rs:581`, `ffi/mod.rs:243`). But flag a **two-slot drift
  risk**: `nip17_local_keys` (`ffi/mod.rs:258`) is a *second* Rust-owned
  key slot, co-maintained with `marmot_local_nsec` — six `update_nsec_slot`
  sites in `dispatch.rs` mirror six identity-mutation points. The two slots
  hold different types (`Zeroizing<String>` nsec vs parsed `nostr::Keys`)
  for different consumers, so there is no bug today — but the next protocol
  crate needing key access must NOT add a third slot. Design one typed
  key-access seam before that happens. This was flagged in #42; it has not
  worsened, but it has not been addressed either.

- **Chirp LOC: 10,743** — up 799 from #42's 9,944. ~36× the 300-LOC
  budget. The DM files themselves are thin and protocol-logic-free; the
  problem is aggregate growth on a broken delivery path.

- **ADR-0026 status: Implemented, zero consumers, 1-cycle clock elapsed.**
  Consume next PR or delete.

- **kind:10050 gap: seam shipped (PR #137), cache + ingest + correct
  fallback all still missing.** `recipient_dm_relays` returns `None`
  forever; `dm.rs` falls back to the wrong relays. This is the single
  highest-value next PR and is fully templatable from `relay_list.rs`.

---

**Bottom line:** The D0 sweep is real, complete, and clean — credit where
due. But PR #137 is a half-fix dressed as a fix: it shipped the kind:10050
*seam* without the kind:10050 *cache*, leaving DM delivery routed to the
wrong relays exactly as #42 found it, now camouflaged behind a `warn!`. Two
of #42's three risks are unchanged; the third (ADR-0026) hit its deadline.
Stop shipping NIP-17 surface. The next PR fills the seam with a real
kind:10050 ingest path (a 98-line clone of `relay_list.rs`) and adds the
one-line bunker placeholder — or DM "works" in the demo and silently loses
every real message.
