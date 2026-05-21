# Opus Direction Review #39

Date: 2026-05-21
Scope: NIP-17 DM readiness, the signer NIP-44 seam, `dispatch_action` consumer pipe,
`HttpCapability`, the `active_local_nsec` slot, NIP-29 post-#119 state.
Ground truth: direct reads of `crates/nmp-core/src/`, `crates/nmp-nip29/`,
`crates/nmp-nip59/`, `crates/nmp-signers/`, `apps/chirp/nmp-app-chirp/src/ffi.rs`,
`docs/decisions/`, `git log`/`git diff origin/master..HEAD`.

---

## Two corrections to the brief, up front

1. **NIP-17 is not "in flight."** The brief says "NIP-17 DM Phase 1 in progress:
   new `crates/nmp-nip17/` ... `ActorCommand::SendGiftWrappedDm` ... actor arm."
   None of that exists. `crates/nmp-nip17/` is absent. `grep -rn SendGiftWrappedDm`
   over `crates/` and `apps/` returns nothing. `git diff origin/master..HEAD --stat`
   is empty; working tree is clean of NIP-17 files. This review treats NIP-17 as
   **not yet started** — which is the right time to review it, before the design
   sets.

2. **`nmp.zap` is already gone.** PR #118 deleted the `register_nip57_actions`
   call. The brief's "Inert surfaces" list still names `nmp.nip57.zaps`; the
   *DomainModule* type still compiles (`nmp-nip57` is a workspace member) but it
   is no longer registered. The remaining genuinely-inert surface is
   `HttpCapability` — see §4. The brief's "wallet" framing is also stale: NWC
   has Swift callers (`ios/Chirp/Chirp/Features/WalletView.swift`,
   `KernelBridge.swift`), so `WalletConnect`/`WalletPayInvoice` are *live*, not
   inert. Do not delete them.

---

## TL;DR

- **The #1 architectural blocker NIP-17 forces is the signer NIP-44 seam, and it
  is not solved.** `RemoteSignerHandle` (`crates/nmp-core/src/remote_signer.rs:20`)
  — the only signer abstraction the actor holds — exposes exactly one crypto
  method: `sign`. It has **no `nip44_encrypt` / `nip44_decrypt`**. The `Nip44`
  trait exists in `nmp-signers` (`signers/traits.rs:41`, impls for local + NIP-46
  + NIP-07) but is **not re-exposed across the D0 boundary**. A
  `SendGiftWrappedDm` actor command therefore *cannot gift-wrap for a bunker
  user*. ADR-0025 said "route DM-send through `SendGiftWrappedDm`, not bespoke
  FFI" — but it never answered "how does the actor do NIP-44 when the signer is
  remote." That is the deliverable NIP-17 must produce first.
- **`nmp_nip59::gift_wrap` takes `&Keys` and is therefore local-keys-only by
  construction.** It is fine for Marmot (ADR-0025 cluster, holds raw `Keys`). It
  must NOT be the NIP-17 send path for bunker users without a degrade story.
- **The `active_local_nsec` slot is a latent security/architecture trap.**
  `NmpApp` holds `Arc<Mutex<Option<Zeroizing<String>>>>` (`ffi/mod.rs:240`) —
  the plaintext nsec, extracted from the actor, readable by any app crate via
  `NmpApp::active_local_nsec()` (`ffi/mod.rs:892`). Marmot already consumes it
  (`marmot/ffi.rs:323`). If NIP-17's send executor reads this slot to call
  `gift_wrap`, the kernel's "actor owns the keys" invariant (D3/D4) is dead and
  every bunker user is silently excluded. **NIP-17 must not touch this slot.**
- **Consumer pipe is healthy.** PR #119 wired `react_in_group` +
  `comment_in_group` into `GroupChatView` — the two surfaces #38 measured as
  inert are now live. Live `dispatch_action` namespaces with a real Swift
  caller: `nmp.publish`, `chirp.react`, `chirp.follow`, `chirp.unfollow`,
  `nip29.post_chat_message`, `nip29.react_in_group`, `nip29.comment_in_group`.
  Live projection: `nip29.group_chat`. **Inert: only `HttpCapability`.** This is
  the best live:inert ratio in the series — 8:1. Credit it.

---

## 1. Highest-risk architectural debt: the signer crypto seam is `sign`-only

`RemoteSignerHandle` is the actor's entire view of a remote signer:

```
crates/nmp-core/src/remote_signer.rs:20
pub trait RemoteSignerHandle: Send + Sync + std::fmt::Debug {
    fn pubkey_hex(&self) -> String;
    fn signer_kind(&self) -> &'static str;
    fn persistence_payload_json(&self) -> Option<String> { None }
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent>;
    fn deliver_rpc_response(&self, response_json: &str);
    fn disconnect(&self) {}
}
```

The richer trait surface exists one crate over but stays there:

```
crates/nmp-signers/src/signers/traits.rs:41   pub trait Nip44 { ... }
crates/nmp-signers/src/signers/nip46/mod.rs:311  impl Nip44 for Nip46Signer  // enqueues "nip44_encrypt"/"nip44_decrypt" RPC
crates/nmp-signers/src/signers/local.rs:287      impl Nip44 for LocalKeySigner
```

So a remote NIP-46 bunker *can* do NIP-44 encrypt/decrypt over the wire — the
capability is built and tested (`nip46/handle.rs:486-527`). The actor just
can't reach it. **This is the gap. It is not new debt — it is debt that was
invisible until NIP-17 needed it.** Marmot dodged it by holding raw `Keys`
inside `MarmotService` (`service.rs:181`), which is exactly the ADR-0025
exception. NIP-17 cannot take that exception — ADR-0025 explicitly forbids it.

**The fix — and the first NIP-17 deliverable — is to extend `RemoteSignerHandle`
with the NIP-44 + seal-sign operations gift-wrap needs**, and have
`nmp-signers`' concrete handle delegate to its already-built `Nip44` impl.
Concretely, add:

```rust
/// NIP-44 encrypt `plaintext` to `recipient`. SignerOp because a remote
/// bunker round-trips an RPC.
fn nip44_encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String>;
fn nip44_decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String>;
```

The local-keys path (the actor's own in-memory identity, not a remote handle)
also needs the same two ops — today the actor signs local identities via
`sign_active_nonblocking` (`actor/commands/identity.rs`), and that path must
gain a parallel `nip44_*` so the *executor code is signer-agnostic*. One unified
"crypto op" enum that resolves to local-sync or remote-async is the clean shape;
it is the same `SignerOp`/`PendingSign` polling machinery the publish path
already uses (D8 — no blocking).

**This must be its own ADR (call it ADR-0026 — "Signer NIP-44 / seal seam")
and its own NIP-17 phase, before any `SendGiftWrappedDm` command lands.** If
`SendGiftWrappedDm` ships first against `nmp_nip59::gift_wrap(&Keys, …)`, it
will be local-keys-only, and the only way to make it "work" for bunker users
will be reading `active_local_nsec` — which doesn't exist for bunker users at
all, so it's a silent D6 failure (no error surfaced, message just never sends).

### Subtlety the ADR must resolve: the seal vs. the outer wrap

NIP-59 gift-wrap is *three* crypto operations, and they do not all belong to the
user's key:

1. **Seal (kind:13):** NIP-44-encrypt the rumor to the recipient, signed by the
   **sender's** key. → needs `RemoteSignerHandle::sign` + `nip44_encrypt`.
2. **Gift-wrap (kind:1059):** NIP-44-encrypt the seal, signed by an **ephemeral,
   throwaway key** generated per-wrap. → the actor generates this `Keys` itself;
   no signer involvement, no D0 issue (an ephemeral key is not identity).
3. The ephemeral key's NIP-44 conversation key is `ephemeral_sk × recipient_pk`
   — pure local computation once the ephemeral key exists.

So the actor can do step 2/3 entirely on its own. Only step 1 needs the
signer. The ADR should say this explicitly: **the signer seam only owes
`sign(kind:13)` + `nip44_encrypt(rumor→recipient)`; the outer wrap is actor-local
crypto.** That keeps the new trait surface to exactly two methods.

---

## 2. The NIP-17 phase plan

The brief's "Phase 2 / Phase 3" framing is too coarse — it hides the signer
seam. Correct split:

- **Phase 1 — `nmp-nip17` crate, pure builders.** New workspace member.
  `kind:14` chat-rumor `UnsignedEvent` builder + typed `DmInput { recipient,
  content, reply_to: Option<String> }`. Zero actor coupling, zero crypto. Thin
  adapter over `nostr`'s NIP-17 module if it has one (per MEMORY: "use
  rust-nostr, not scratch crypto" — check `nostr::nips::nip17` before
  hand-rolling the kind:14 shape). Mirrors `nmp-nip29/src/action/content.rs`.
- **Phase 2 — Signer NIP-44 seam (ADR-0026).** Extend `RemoteSignerHandle` +
  the local-keys path with `nip44_encrypt`/`nip44_decrypt` (and confirm `sign`
  can produce a kind:13 seal — it can; `sign` is kind-agnostic). **This is the
  gating phase.** Nothing downstream is correct without it.
- **Phase 3 — `ActorCommand::SendGiftWrappedDm { rumor, recipients }` + arm.**
  The actor: for each recipient (and a self-copy), builds the seal via the
  Phase-2 seam, generates an ephemeral key, wraps, emits a
  `PublishUnsignedEventToRelays`-shaped publish pinned to the recipient's NIP-17
  DM relays (kind:10050). The self-copy is mandatory per NIP-17 — the sender
  cannot otherwise see their own sent DMs. `correlation_id` threads through for
  `action_results` (the publish engine already array-shapes this —
  `publish_engine.rs:444`).
- **Phase 4 — `DmInboxProjection` (a `KernelEventObserver`).** On each inbound
  kind:1059, NIP-44-decrypt via the Phase-2 seam, and if the inner rumor is
  kind:14, accumulate a `DmMessage` list keyed by conversation pubkey. Register
  it via `register_snapshot_projection("nip17.dm_inbox", …)` — copy
  `GroupChatProjection` exactly (`crates/nmp-nip29/src/projection/mod.rs`). **Do
  NOT model this as a `DomainModule`** — that trait family is dormant (every
  prior review since #19 said so; PR #116 just deleted `WelcomeUnwrapModule`,
  the last `DomainModule` shell, for being exactly this).
- **Phase 5 — Swift `DmListView` + `DmConversationView` + bridge.**
  Mechanically a copy of PR #115's `GroupChatView`/`GroupChatStore`/`GroupChatBridge`.

### The kind:1059 multiplexing problem (Phase 4 must answer this)

`nmp-marmot` already registers a `kind:1059 #p=self` interest for inbound
Welcomes (`nmp-marmot/src/interest.rs`, per #38 §2). A `DmInboxProjection`
subscribes to *the same kind*. So two observers see every kind:1059. Today
Marmot owns it; NIP-17 must not blindly add a second decrypt pass — that's
*double NIP-44 cost per inbound wrap*, and on a remote bunker that is two RPC
round-trips per gift-wrap. **Decide and document:** single decrypt pass in one
observer that dispatches by inner-rumor kind (kind:444 → Marmot, kind:14 → DM
inbox), or accept the double cost with a comment saying why. The single-pass
demux is correct — but it means the kind:1059 observer becomes shared
infrastructure, not NIP-17-owned. Put this in ADR-0026 too; it is the same
"who owns the gift-wrap inbox" question.

---

## 3. Patterns that are inconsistent or wrong

### 3a. `#[allow(dead_code)]` on `PublishUnsignedEventToRelays` is a lie

`crates/nmp-core/src/actor/mod.rs:278` marks `PublishUnsignedEventToRelays`
`#[allow(dead_code)]`. It is **not dead.** `dispatch.rs:324` matches it, and
`nmp-nip29`'s `PublishPlan::into_actor_command` (`action/publish_plan.rs:100`)
constructs it — that is the live path for all three NIP-29 chat actions. The
attribute is stale from before PR #109/#110. **Fix: delete the
`#[allow(dead_code)]`.** It now actively misleads — a reader greps for dead code,
finds this, and wastes time. Same audit needed for `AddRemoteSigner` /
`RemoveRemoteSigner` / `BunkerHandshakeProgress` (`mod.rs:196/202/210`): those
*are* genuinely test-only today, so their attributes are honest — but they
should carry the comment "constructed only by the broker crate (Stage 4)" so
the distinction from the `PublishUnsignedEventToRelays` false-positive is
explicit.

### 3b. `nmp_nip59::gift_wrap` doc says "post-v1 ... `KeyringCapability`" — that future never came

`wrap.rs:28-33` promises "In the post-v1 Marmot flow the actor's signer-bridge
will hold keys via `KeyringCapability`." There is no `KeyringCapability`
gift-wrap path; Marmot holds raw `Keys`. The docstring describes an architecture
that was never built. **Fix: rewrite the seam note** to state the real
contract — `gift_wrap` is a local-keys primitive, callers that need
remote-signer support must go through the Phase-2 seam (ADR-0026), and Marmot is
the named ADR-0025 exception that may keep calling it directly.

### 3c. `active_local_nsec` is a key-exfiltration channel with no access control

`ffi/mod.rs:240` — `Arc<Mutex<Option<Zeroizing<String>>>>` — the plaintext
nsec, lifted out of the actor, handed to *any* app crate via the public
`NmpApp::active_local_nsec()` (`ffi/mod.rs:892`). `Zeroizing` wipes it on drop,
which is good hygiene, but it does not change the architectural fact: **the
kernel's "actor is the sole key holder" invariant is already breached.** It
exists solely so `nmp_app_chirp_marmot_register_active` can hand the key to
`MarmotService`. That is the ADR-0025 cluster and is accepted. But the slot is
*not scoped to Marmot* — it is a general escape hatch. **Recommendation:** do
not delete it (Marmot needs it), but (a) rename it to `marmot_local_nsec` and
document it as ADR-0025-scoped, and (b) add a one-line note to ADR-0025 that
this slot is part of the bounded exception and **NIP-17 must not read it.**
Without that note, the next agent building NIP-17's send executor will find
`active_local_nsec()`, see it returns `Keys`, and wire `gift_wrap` to it in
twenty minutes — shipping a bunker-broken DM feature that passes every test
(tests use local keys).

### 3d. `HttpCapability` is defined, tested, and registered nowhere

`crates/nmp-core/src/substrate/http.rs` defines `HttpCapability`,
`HttpCapabilityWiring`, the lot — with unit tests. `grep HttpCapability
apps/chirp/.../ffi.rs` and `marmot/ffi.rs` → nothing. It is not registered by
any host. It is the lone genuinely-inert surface. ADR-0023/0024 cover the
sync-socket and async-capability protocols it would need; ADR-0024 was 0/5 done
as of #36 and nothing has moved. **This has been inert for 3+ reviews.
Recommendation: delete `substrate/http.rs` + the `HttpCapability*` re-exports
in `substrate/mod.rs:55`.** It is ~200 lines of shipped-but-inert code whose
only consumer (NIP-57 zap LNURL) was itself deleted in PR #118. When zaps come
back, rebuild the capability against a real consumer — not before. Keeping it
is the exact "shipped lie camouflaged by green CI" pathology #33/#36 named.

---

## 4. What to build next, after NIP-17 DMs

NIP-17 is correctly the next feature. After it ships end-to-end (all 5 phases,
a working Swift DM screen):

1. **NIP-17 read receipts / typing — NO.** Skip. Scope creep.
2. **Promote the Phase-2 signer seam into the Marmot path.** Once
   `RemoteSignerHandle` can do NIP-44, `MarmotService` no longer *needs* raw
   `Keys` for gift-wrap — only MDK's MLS state genuinely needs handle-scoped
   crypto. This is the path to *shrinking* the ADR-0025 exception: Welcome
   gift-wrap could move onto the seam, leaving only the MLS group handle as the
   bespoke part. That is real debt reduction and the highest-leverage
   post-NIP-17 work — it makes `active_local_nsec` deletable.
3. **NIP-57 zaps, rebuilt against a real `HttpCapability` consumer** — only
   after ADR-0024's async-capability protocol is actually implemented. Not
   before. If zaps come back before ADR-0024 is real, they will be inert again.

Do **one** of these, finish it, then review. Do not register all three.

---

## 5. What to stop / delete this cycle

- **Delete `crates/nmp-core/src/substrate/http.rs`** + `HttpCapability*`
  re-exports (`substrate/mod.rs:55`). Inert 3+ reviews, only consumer deleted.
- **Delete `#[allow(dead_code)]` on `ActorCommand::PublishUnsignedEventToRelays`**
  (`actor/mod.rs:278`) — it is live; the attribute misleads.
- **Do NOT delete:** NWC/wallet (has Swift callers), `nmp-nip57` crate (zap
  *receipt* decode is still a valid read-side primitive even with the action
  unregistered), `active_local_nsec` (Marmot needs it — but rename + document).
- **Stop** writing docstrings that describe unbuilt futures (`wrap.rs` seam
  note, §3b). A docstring promising `KeyringCapability` that never shipped is
  worse than no docstring — it sends readers chasing a phantom.

---

## Verdict

The consumer pipe is genuinely healthy (8:1 live:inert) — the longest-running
pathology of this review series is resolved. The new risk is narrower and
sharper: **NIP-17 will be built, and if the signer NIP-44 seam is not made a
named, gating phase first, it will ship local-keys-only and silently fail for
every bunker user.** The brief's "Phase 1 in progress" is fiction — nothing has
landed — which means there is still time to do this right. The single most
important sentence in this review: **write ADR-0026 (signer NIP-44 / seal seam)
before `ActorCommand::SendGiftWrappedDm`, and forbid the NIP-17 send executor
from reading `active_local_nsec`.**
