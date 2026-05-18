# MDK API Spike — nmp-marmot

**Spike date**: 2026-05-18  
**MDK versions examined**: mdk-core 0.8.0, mdk-sqlite-storage 0.8.0, mdk-storage-traits 0.8.0, openmls 0.8.1  
**Spike scope**: cargo-deny gate + exact API surface for nmp-marmot implementation planning

---

## GO / NO-GO Verdict

**CONDITIONAL GO with pre-existing deny.toml debt.**

The mdk-core 0.8.0 + mdk-sqlite-storage 0.8.0 + openmls 0.8.1 transitive graph compiles cleanly (`cargo build -p nmp-marmot` exits 0). The cargo-deny errors are **pre-existing** workspace-level failures unrelated to the MDK addition. See the Cargo-Deny Gate section for the exact breakdown and proposed deny.toml additions.

---

## 1. Cargo-Deny Gate

### Build Result

```
cargo build -p nmp-marmot   →   Finished `dev` profile [unoptimized + debuginfo] in 4m 54s
```

Build passes. No errors. No warnings from the nmp-marmot crate itself.

### `cargo deny check licenses bans sources advisories` — Full Verdict

Exit code: **5** (errors present).

**Three `error[rejected]` license failures — ALL pre-existing, NOT from MDK:**

| Crate | License | Pull path |
|---|---|---|
| `clipboard-win v5.4.1` | `BSL-1.0` | `nmp-desktop` → `eframe` → `arboard` → `clipboard-win` AND `nmp-repl` → `rustyline` → `clipboard-win` |
| `error-code v3.3.2` | `BSL-1.0` | same path via `clipboard-win` |
| `epaint_default_fonts v0.29.1` | `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` | `nmp-desktop` → `eframe` → `egui` → `epaint` |

These three failures existed before this spike. The MDK addition (mdk-core, mdk-sqlite-storage, nostr, openmls and their transitive graph) introduces **zero new license rejections**. All MDK-path crates carry MIT, Apache-2.0, BSD-2-Clause, or dual-licensed permissive identifiers, all of which are in `deny.toml`'s allow-list.

**Two `error[unmaintained]` advisory failures — one new (MDK-path), one pre-existing:**

| Advisory | Crate | Pull path | New? |
|---|---|---|---|
| RUSTSEC-2024-0384 | `instant v0.1.13` | `nostr v0.44.2` → `instant` (wasm32 target dep) | **Pre-existing** — `nostr v0.44.2` was already in the workspace via `nmp-core` before this spike. `instant v0.1.13` is present in the Cargo.lock prior to MDK addition. MDK adds more consumers of the same `nostr v0.44.2` node but introduces no new lockfile entry for `instant`. |
| RUSTSEC-2024-0436 | `paste v1.0.15` | `nmp-testing`, `nmp-desktop` → `accesskit_windows` | Pre-existing |

Note: `instant v0.1.13` is a **wasm32-only** target dependency of `nostr v0.44.2`. It is never compiled in native builds. cargo-deny still flags it because it appears in the lockfile.

**Warnings (all pre-existing duplicate-version churn):**

`bans.multiple-versions = "warn"` generates 40+ warnings for bitflags, getrandom, rand, thiserror, quick-xml, windows-sys, objc2 family, etc. These all originate from `nmp-desktop` (egui/eframe/winit stack) and pre-date this spike.

### Proposed deny.toml additions (for orchestrator to apply)

The orchestrator MUST resolve the three pre-existing license failures independently. The MDK-specific advisory entry to add is:

```toml
# [advisories] ignore additions:

# RUSTSEC-2024-0384: instant 0.1.13 is unmaintained.
# Pulled by nostr v0.44.2 as a wasm32-only target dep (not compiled on native).
# nostr 0.44.x is the latest stable nostr crate; no upgrade path exists yet.
# Risk: wasm builds only; nmp-marmot does not target wasm.
{ id = "RUSTSEC-2024-0384", reason = "wasm32-only transitive dep of nostr 0.44.x; not compiled on native targets; no upstream fix available as of 2026-05-18" },
```

The pre-existing failures (BSL-1.0 for clipboard-win/error-code, OFL-1.1 + LicenseRef-UFL-1.0 for epaint_default_fonts, RUSTSEC-2024-0436 for paste) require separate deny.toml additions outside this spike's scope.

### MDK-specific cargo-deny verdict: PASS (no new hard failures)

The three license errors and the `paste` advisory are pre-existing. The `instant` advisory is a lockfile artifact from the pre-existing `nostr v0.44.2` dependency, not new to this spike.

---

## 2. MDK Top-Level Type and Storage Init

### `MDK<Storage>` struct

```rust
// crates/nmp-marmot/... (at call site):
use mdk_core::{MDK, MdkConfig};
use mdk_sqlite_storage::MdkSqliteStorage;

// Production (encrypted, platform keyring):
let storage = MdkSqliteStorage::new(
    "/path/to/marmot-mls-state.sqlite",
    "com.example.myapp",         // service_id
    "mdk.db.key.default",        // db_key_id
)?;
let mdk: MDK<MdkSqliteStorage> = MDK::new(storage);

// Or with custom config:
let mdk = MDK::builder(storage)
    .with_config(MdkConfig { max_past_epochs: 5, ..Default::default() })
    .build();
```

**SQLite file path pattern for nmp-marmot:** a dedicated file alongside NMP's LMDB store, e.g. `<app_support_dir>/marmot-mls-state.sqlite`. The file is an implementation detail of the `nmp-marmot` crate; no other NMP crate sees it.

**Storage trait:** `mdk_storage_traits::MdkStorageProvider`. `MdkSqliteStorage` implements it. The three constructors are:
- `MdkSqliteStorage::new(path, service_id, db_key_id)` — encrypted, keyring-managed (production)
- `MdkSqliteStorage::new_with_key(path, EncryptionConfig)` — encrypted, caller-supplied key
- `MdkSqliteStorage::new_unencrypted(path)` — dev/test only (feature-gated: `test-utils`)
- `MdkSqliteStorage::new_in_memory()` — in-memory, dev/test only

---

## 3. Exact API — Key Operations

All methods are `&self` (interior mutability via `Arc<Mutex<Connection>>`). All return `Result<T, mdk_core::Error>`.

### 3.1 Key Package Generation

```rust
// Produces KeyPackageEventData for publishing as kind:30443 (and dual-publish kind:443 through May 31 2026)
pub fn create_key_package_for_event<I>(
    &self,
    public_key: &nostr::PublicKey,   // caller's Nostr pubkey (binds MLS credential)
    relays: I,                        // I: IntoIterator<Item = nostr::RelayUrl>
) -> Result<KeyPackageEventData, Error>

pub struct KeyPackageEventData {
    pub content: String,          // base64-encoded TLS-serialized MLS KeyPackage
    pub tags_30443: Vec<Tag>,     // use with Kind::Custom(30443) — includes d tag
    pub tags_443: Vec<Tag>,       // use with Kind::Custom(443) — legacy, no d tag
    pub hash_ref: Vec<u8>,        // postcard-serialized KeyPackageRef for lifecycle tracking
    pub d_tag: String,            // 32-byte hex; store and reuse on rotation for relay replacement
}
```

The caller wraps `content` + `tags_30443` into a Nostr event and signs it with the Nostr key. MDK does NOT sign Nostr events; it produces the content and tags. The Nostr event signature is the caller's responsibility (nmp-marmot uses M6 signer surface).

### 3.2 Create Group

```rust
pub fn create_group(
    &self,
    creator_public_key: &PublicKey,
    member_key_package_events: Vec<Event>,  // already-signed kind:30443/443 events fetched from relay
    config: NostrGroupConfigData,
) -> Result<GroupResult, Error>

pub struct GroupResult {
    pub group: mdk_storage_traits::groups::types::Group,
    pub welcome_rumors: Vec<nostr::UnsignedEvent>,  // kind:444, one per invitee; caller wraps in NIP-59 gift-wrap
}

pub struct NostrGroupConfigData {
    pub name: String,
    pub description: String,
    pub image_hash: Option<[u8; 32]>,
    pub image_key: Option<[u8; 32]>,
    pub image_nonce: Option<[u8; 12]>,
    pub relays: Vec<RelayUrl>,
    pub admins: Vec<PublicKey>,
}
```

After `create_group`, the caller MUST call `merge_pending_commit(group_id)` to advance the epoch.

### 3.3 Add Member (Invite)

```rust
pub fn add_members(
    &self,
    group_id: &GroupId,
    key_package_events: &[Event],   // kind:30443/443 events for each invitee
) -> Result<UpdateGroupResult, Error>

pub struct UpdateGroupResult {
    pub evolution_event: Event,              // kind:445, ready to publish to group relay
    pub welcome_rumors: Option<Vec<UnsignedEvent>>,  // kind:444 rumors; wrap in NIP-59
    pub mls_group_id: GroupId,
}
```

Admin-only. After publishing `evolution_event`, call `merge_pending_commit(group_id)`.

### 3.4 Process Welcome (Receiver side)

```rust
// Step 1: Unwrap NIP-59 gift-wrap → extract rumor (UnsignedEvent, kind:444)
// Step 2:
pub fn process_welcome(
    &self,
    wrapper_event_id: &nostr::EventId,  // ID of the outer kind:1059 gift-wrap event
    rumor_event: &UnsignedEvent,         // the unwrapped kind:444 rumor
) -> Result<welcome_types::Welcome, Error>

// Step 3: Accept (finalizes the MLS group join):
pub fn accept_welcome(
    &self,
    welcome: &welcome_types::Welcome,
) -> Result<(), Error>
```

After `accept_welcome`, the group is `Active` and the member should immediately call `self_update` per MIP-02 (MDK tracks this as `SelfUpdateState::Required`).

### 3.5 Send Message

```rust
pub fn create_message(
    &self,
    mls_group_id: &GroupId,
    rumor: UnsignedEvent,            // the plaintext message as an UnsignedEvent (kind:9 or similar)
    tags: Option<Vec<EventTag>>,     // optional NIP-40 expiration etc.
) -> Result<nostr::Event, Error>
// Returns a signed kind:445 Event ready for publication to the group relay.
```

MDK handles MLS encryption (MLS ApplicationMessage) + MIP-03 outer ChaCha20-Poly1305 wrapping. The returned `Event` is already signed (MDK uses the MLS credential key for the outer Nostr event).

### 3.6 Process Incoming Message

```rust
pub fn process_message(
    &self,
    event: &nostr::Event,           // incoming kind:445 event from relay
) -> Result<MessageProcessingResult, Error>

// With MLS sender leaf index context:
pub fn process_message_with_context(
    &self,
    event: &Event,
) -> Result<MessageProcessingOutcome, Error>

pub enum MessageProcessingResult {
    ApplicationMessage(message_types::Message),  // decrypted app message
    Proposal(UpdateGroupResult),                  // proposal auto-committed (admin path)
    PendingProposal { mls_group_id: GroupId },    // proposal stored, waiting admin commit
    IgnoredProposal { mls_group_id: GroupId, reason: String },
    ExternalJoinProposal { mls_group_id: GroupId },
    Commit { mls_group_id: GroupId },             // commit processed, epoch advanced
    Unprocessable { mls_group_id: GroupId },      // out-of-order, expired, etc.
    PreviouslyFailed { mls_group_id: GroupId },
}
```

### 3.7 Self-Update / UpdateKeys

```rust
pub fn self_update(
    &self,
    group_id: &GroupId,
) -> Result<UpdateGroupResult, Error>
// Returns evolution_event (kind:445 commit) to publish. Then merge_pending_commit.
// Any member may call this. Rotates MLS leaf keypair (forward secrecy).
```

### 3.8 Leave Group (Self-Removal)

```rust
pub fn leave_group(
    &self,
    group_id: &GroupId,
) -> Result<UpdateGroupResult, Error>
// Sends a SelfRemove proposal (kind:445). No merge_pending_commit needed —
// self-remove proposals are auto-committed by any other member.
// Falls back to legacy Remove proposal for pre-0.8 groups.
```

### 3.9 Remove Member (Admin-only)

```rust
pub fn remove_members(
    &self,
    group_id: &GroupId,
    pubkeys: &[nostr::PublicKey],    // Nostr pubkeys of members to remove
) -> Result<UpdateGroupResult, Error>
// Admin-only. Returns evolution_event (kind:445 commit). Then merge_pending_commit.
// Atomically removes the member from the admin list within the same commit.
```

### 3.10 Merge Pending Commit

```rust
pub fn merge_pending_commit(
    &self,
    group_id: &GroupId,
) -> Result<(), Error>
// Must be called after create_group, add_members, remove_members, self_update.
// Advances epoch, saves new exporter secrets, syncs group metadata.
```

---

## 4. Nostr Event Kinds

| Kind | Constant | Usage |
|---|---|---|
| `Kind::Custom(30443)` | `MLS_KEY_PACKAGE_KIND` | KeyPackage (addressable, NIP-33). CURRENT spec. |
| `Kind::Custom(443)` | `MLS_KEY_PACKAGE_KIND_LEGACY` | KeyPackage legacy. Dual-publish through 2026-05-31. |
| `Kind::MlsWelcome` (444) | — | Welcome message (kind:444 rumor wrapped in NIP-59 kind:1059) |
| `Kind::Custom(445)` | — | Group message / commit / proposal (encrypted MLS + MIP-03 outer layer) |

**MDK returns ready-to-publish Nostr `Event` objects** for kind:445 (`create_message`, `add_members`, `remove_members`, `self_update`, `leave_group`). For kind:30443/443 KeyPackages, MDK returns `KeyPackageEventData` (content + tags); the caller creates and signs the Nostr event using the Nostr signer. For kind:444 Welcome messages, MDK returns `Vec<UnsignedEvent>` (rumor); the caller wraps each in a NIP-59 gift-wrap and signs it.

**MDK does NOT return signed Nostr events for KeyPackage and Welcome messages** — it returns the MLS-level payload and Nostr tags; the NIP-59 gift-wrap and final Nostr event signing are the caller's responsibility.

---

## 5. 0.7 → 0.8 Breaking-Change Deltas (Affecting nmp-marmot Implementation)

### 5.1 KeyPackage event kind: `kind:443` → `kind:30443` (MUST dual-publish)

`create_key_package_for_event` now returns `KeyPackageEventData` (struct) instead of a tuple. Contains both `tags_30443` and `tags_443`. **Callers MUST dual-publish both** `kind:30443` (for new clients) and `kind:443` (legacy) through 2026-05-31.

- Old: `(String, Vec<Tag>, Vec<u8>)` tuple
- New: `KeyPackageEventData { content, tags_30443, tags_443, hash_ref, d_tag }`

### 5.2 `create_group` — LCD RequiredCapabilities

`create_group` no longer unconditionally requires `SelfRemove` in `RequiredCapabilities`. It now computes a least-common-denominator (LCD) intersection from all invitees' advertised proposals. An all-modern group still gets `[SelfRemove]`; any legacy invitee strips it. Empty-invitee groups get `[]` to stay open for later `add_members` with legacy key packages.

**Impact on nmp-marmot**: `CreateGroup` ActionModule must supply `NostrGroupConfigData` correctly; no direct LCD handling needed. MDK handles it internally.

### 5.3 `leave_group` — SelfRemove proposal (not Remove)

`leave_group` now emits a `SelfRemove` proposal (MLS Extensions type `0x000a`) instead of a `Remove` proposal. Any member can auto-commit it (no admin required). Falls back to `Remove` for legacy `PURE_CIPHERTEXT` groups.

**Impact on nmp-marmot**: `LeaveGroup` ActionModule publishes the `evolution_event`; no `merge_pending_commit` required for SelfRemove path.

### 5.4 Wire format: MIXED_CIPHERTEXT (not PURE_CIPHERTEXT)

New groups use `MIXED_CIPHERTEXT` wire format policy. Required to accept `PublicMessage` SelfRemove proposals from departing members.

### 5.5 MIP-03 encryption change (0.6 → 0.7, already in 0.8)

kind:445 content is `base64(nonce || ChaCha20-Poly1305-ciphertext)` — NOT NIP-44. The outer encryption key is derived via `MLS-Exporter("marmot", "group-event", 32)`. This is baked into `create_message` / `process_message`; nmp-marmot does not touch this layer directly.

**Legacy compatibility deadline**: MIP-03 legacy NIP-44 fallback expired 2026-05-15. Any group messages encrypted with pre-0.7.0 secrets will fail to decrypt in 0.8.x.

### 5.6 Error variants (exhaustive matchers must add arms)

New in 0.8.0:
- `Error::NotAdmin` (replaces stringly `Error::Group("not admin")` for add/remove/demote)
- `Error::InviteeMissingRequiredProposal` (invitee's LeafNode.capabilities.proposals too narrow)

Removed: `GROUP_CONTEXT_REQUIRED_PROPOSALS` constant, `MDK::required_capabilities_extension()` helper.

### 5.7 MIP-05 token wire format (breaking but optional feature)

`TOKEN_PLAINTEXT_LEN` increased 220→1024 bytes. `MAX_NOTIFICATION_REQUEST_TOKENS` decreased 100→25. `InvalidApnsTokenLength` / `InvalidFcmTokenLength` replaced by `InvalidDeviceTokenLength`. nmp-marmot does not use the `mip05` feature for this milestone; this is a NO-OP.

---

## 6. MDK → NMP Mapping Table

### DomainModules

| NMP DomainModule | MDK calls |
|---|---|
| `MarmotGroup` | `MDK::get_group(&GroupId)` → `group_types::Group` for display metadata. `MDK::get_members(&GroupId)` for member set. The actual MLS ratchet state lives in MDK/SQLite, not in NMP's LMDB. |
| `MarmotMessage` | `MDK::process_message(&Event)` → `MessageProcessingResult::ApplicationMessage(msg)`. `MDK::get_messages(&GroupId, pagination)` for history. |
| `MarmotKeyPackage` | `MDK::create_key_package_for_event(&PublicKey, relays)` → `KeyPackageEventData`. Track `hash_ref` and `d_tag` for rotation lifecycle. `MDK::parse_key_package(&Event)` for peer key packages fetched from relay. |
| `MarmotWelcome` | `MDK::process_welcome(wrapper_id, rumor)` → `welcome_types::Welcome`. `MDK::get_pending_welcomes(pagination)` for pending list. |

### ViewModules

| NMP ViewModule | MDK calls |
|---|---|
| `GroupList` | `MDK::get_groups()` → `Vec<group_types::Group>`. Filter by `group.state == GroupState::Active`. Unread count derived from `group.last_message_at` vs client-tracked read cursor. |
| `GroupMessages` | `MDK::get_messages(&GroupId, Some(Pagination))` → paginated `Vec<message_types::Message>`. Live-update on new epoch from `MessageProcessingResult::Commit`. |
| `MemberList` | `MDK::get_members(&GroupId)` → `BTreeSet<PublicKey>`. `MDK::group_leaf_map(&GroupId)` → `BTreeMap<u32, PublicKey>` for leaf indices. |

### ActionModules

| NMP ActionModule | MDK calls |
|---|---|
| `PublishKeyPackage` | `mdk.create_key_package_for_event(pubkey, relays)` → wrap in `EventBuilder` + sign with M6 signer → publish `kind:30443` + dual `kind:443` |
| `CreateGroup` | `mdk.create_group(creator_pk, member_kp_events, config)` → `GroupResult { group, welcome_rumors }` → wrap each rumor in NIP-59 gift-wrap → publish commit + send gift-wraps → `mdk.merge_pending_commit(&group_id)` |
| `InviteMember` | Fetch target's `kind:30443` KeyPackage event from relay → `mdk.add_members(&group_id, &[kp_event])` → `UpdateGroupResult { evolution_event, welcome_rumors }` → publish `evolution_event` to group relay → wrap `welcome_rumors` in NIP-59 → `mdk.merge_pending_commit(&group_id)` |
| `SendMessage` | `mdk.create_message(&group_id, rumor, tags)` → `Event` → publish to group relay |
| `LeaveGroup` | `mdk.leave_group(&group_id)` → `UpdateGroupResult { evolution_event, .. }` → publish `evolution_event` to group relay (no `merge_pending_commit` for SelfRemove path) |
| `RemoveMember` | `mdk.remove_members(&group_id, &[pubkey])` → `UpdateGroupResult { evolution_event, .. }` → publish → `mdk.merge_pending_commit(&group_id)` |
| `UpdateKeys` | `mdk.self_update(&group_id)` → `UpdateGroupResult { evolution_event, .. }` → publish → `mdk.merge_pending_commit(&group_id)` |

---

## 7. Key Implementation Notes for nmp-marmot

1. **MDK is not async.** All methods are synchronous `&self`. nmp-marmot should call MDK from a dedicated blocking thread (tokio `spawn_blocking` or equivalent).

2. **Nostr event creation for KeyPackage and Welcome requires the caller's Nostr signer.** MDK provides content+tags; nmp-marmot uses M6 signer to sign the resulting EventBuilder. For kind:445 group messages, MDK returns a fully signed `Event` using the MLS credential key — no additional signing needed.

3. **`merge_pending_commit` is mandatory** after `create_group`, `add_members`, `remove_members`, and `self_update`. Skipping it leaves MLS in an inconsistent state. For `leave_group` (SelfRemove path), the commit is performed by a peer; the leaving member does NOT call `merge_pending_commit`.

4. **Dual-publish KeyPackages through 2026-05-31.** The `d_tag` field in `KeyPackageEventData` must be stored and reused on rotation to enable relay-side event replacement for `kind:30443`.

5. **Post-join self-update is mandatory per MIP-02.** MDK sets `SelfUpdateState::Required` after `accept_welcome`. `nmp-marmot` should invoke `UpdateKeys` ActionModule immediately after joining. Use `mdk.groups_needing_self_update(threshold_secs)` to surface groups needing rotation.

6. **`MdkSqliteStorage::new_unencrypted` is feature-gated.** The `test-utils` feature must be enabled in `[dev-dependencies]` for tests. Production code must use `::new(path, service_id, db_key_id)` with platform keyring.

7. **`clear_pending_commit` for failed publishes.** If the relay publish of `evolution_event` fails, the caller must invoke `mdk.clear_pending_commit(&group_id)` to unblock future group operations.

8. **Ciphersuite**: `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519` (0x0001) — the only supported ciphersuite. Hardcoded in `DEFAULT_CIPHERSUITE`.

9. **openmls is a transitive dep only.** nmp-marmot should not directly import `openmls` types. All openmls types that appear in MDK's public API surface are re-exported via `mdk_core::prelude` (e.g., `ProposalType`, `ExtensionType`, `VerifiableCiphersuite`).

---

## 8. Dependency Graph Summary (MDK additions only)

New crates added by mdk-core + mdk-sqlite-storage not previously in the workspace:

- `mdk-core 0.8.0` (MIT), `mdk-sqlite-storage 0.8.0` (MIT), `mdk-storage-traits 0.8.0` (MIT), `mdk-macros 0.8.0` (MIT)
- `openmls 0.8.1` (MIT), `openmls_traits 0.5.0` (MIT), `openmls_basic_credential 0.5.0` (MIT), `openmls_rust_crypto 0.5.1` (MIT), `openmls_sqlite_storage 0.2.0` (MIT), `openmls_memory_storage 0.5.0` (MIT), `openmls_libcrux_crypto 0.3.1` (MIT)
- `rusqlite 0.37.0` (MIT), `libsqlite3-sys 0.35.0` (MIT), `refinery 0.9.0` (MIT), `keyring-core 1.0.0` (MIT OR Apache-2.0)
- `tls_codec 0.4.2` (Apache-2.0 OR MIT), `hpke-rs 0.6.1`, `hpke-rs-rust-crypto 0.6.1`, `hpke-rs-crypto 0.6.1`
- Various crypto primitives (all MIT or Apache-2.0): `chacha20poly1305`, `chacha20`, `p256`, `p384`, `libcrux-*`, `curve25519-dalek`, `ed25519-dalek`, `x25519-dalek`
- `nostr 0.44.2` (MIT) — was already in workspace via `nmp-core`; now also directly referenced by mdk-core/mdk-sqlite-storage (same version, no duplication)
- `image 0.25.10` (MIT OR Apache-2.0) — pulled by mdk-core for MIP-04 media processing (used even without the `mip04` feature for EXIF handling)

All MDK-path licenses are MIT, Apache-2.0, or dual-licensed permissive. No GPL, AGPL, or LGPL in the MDK subgraph.
