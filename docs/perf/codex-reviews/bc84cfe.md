Reading additional input from stdin...
2026-05-18T02:29:22.043013Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e38ea-a3fd-7cb0-b927-51324599cd53
--------
user
Reviewing two NMP master commits: 7d16b3f (T29 M3 phase-1 follow-up addressing 6 codex findings from afc5475) + bc84cfe (T30 M2 phase-1 follow-up addressing 8 codex findings from 92b8260).

NMP doctrine D0-D8 canonical per docs/product-spec/overview-and-dx.md §1.5.
File size: 300 LOC soft, 500 hard.

T29 was tasked to: P1 split mem.rs (1067 LOC HARD-CEILING violation); P1 D4 wire ingest through EventStore; P2 RawEvent → VerifiedEvent with nostr crate; P2 duplicate replaceable provenance merge; P2 kind:5 tombstone max-merge; P2 D8/GC ceilings + BTreeSet idempotency; P3 LMDB skeleton honesty.

T30 was tasked to: P1 Rule 1 wildcard bug (correctness regression); P1 plan-id stability off-spec; P1 remove RoutingSource::Indexer 5th-lane regression; P2 direction table for #p/active-account-read/IndexerProbe; P2 audit assertion strength (300→1000); P2 mod.rs minimal export; P3 file-size splits; P3 TODO removal.

The merges:

=== T29 (M3 follow-up): 7d16b3f ===
 Cargo.lock                                    |  603 ++++++++++++++
 crates/nmp-core/Cargo.toml                    |    1 +
 crates/nmp-core/src/kernel/ingest.rs          |   60 +-
 crates/nmp-core/src/kernel/mod.rs             |   11 +-
 crates/nmp-core/src/kernel/nostr.rs           |    4 +
 crates/nmp-core/src/store/events.rs           |    9 +-
 crates/nmp-core/src/store/lmdb.rs             |    6 +-
 crates/nmp-core/src/store/mem.rs              | 1105 -------------------------
 crates/nmp-core/src/store/mem/domain.rs       |   79 ++
 crates/nmp-core/src/store/mem/gc.rs           |  194 +++++
 crates/nmp-core/src/store/mem/insert.rs       |  357 ++++++++
 crates/nmp-core/src/store/mem/mod.rs          |  193 +++++
 crates/nmp-core/src/store/mem/query.rs        |  394 +++++++++
 crates/nmp-core/src/store/mem/store_impl.rs   |  181 ++++
 crates/nmp-core/src/store/mem/tests.rs        |  132 +++
 crates/nmp-core/src/store/mod.rs              |    6 +-
 crates/nmp-core/src/store/types.rs            |  343 --------
 crates/nmp-core/src/store/types/errors.rs     |   80 ++
 crates/nmp-core/src/store/types/events.rs     |  188 +++++
 crates/nmp-core/src/store/types/gc.rs         |   67 ++
 crates/nmp-core/src/store/types/ids.rs        |   36 +
 crates/nmp-core/src/store/types/mod.rs        |   20 +
 crates/nmp-core/src/store/types/outcomes.rs   |   68 ++
 crates/nmp-core/src/store/types/watermark.rs  |   43 +
 crates/nmp-testing/src/store_harness.rs       |    8 +-

=== T30 (M2 follow-up): bc84cfe ===
 crates/nmp-core/src/kernel/requests/profile.rs     |   2 +-
 crates/nmp-core/src/kernel/requests/thread.rs      |   2 +-
 crates/nmp-core/src/planner/compiler.rs            | 498 ---------------------
 crates/nmp-core/src/planner/compiler/mailbox.rs    | 105 +++++
 crates/nmp-core/src/planner/compiler/mod.rs        | 191 ++++++++
 crates/nmp-core/src/planner/compiler/partition.rs  | 258 +++++++++++
 crates/nmp-core/src/planner/compiler/plan_id.rs    | 159 +++++++
 crates/nmp-core/src/planner/interest.rs            |   4 +-
 .../src/planner/{lattice.rs => lattice/mod.rs}     | 214 +++------
 crates/nmp-core/src/planner/lattice/rules.rs       | 136 ++++++
 crates/nmp-core/src/planner/mod.rs                 |  35 +-
 crates/nmp-core/src/planner/plan.rs                |  54 ++-
 crates/nmp-testing/tests/m2_plan_id_stability.rs   | 225 ++++++++++
 .../tests/m2_subscription_compilation_audit.rs     |  53 ++-
 14 files changed, 1254 insertions(+), 682 deletions(-)

=== T29 commit ===
7d16b3f fix(m3): codex follow-up — mem.rs split + sig verify + ingest wired + GC ceiling + tombstones (T29)
P1 — Article I hard-ceiling fix:
- Delete monolithic store/mem.rs (1105 LOC) → split into mem/{mod,insert,query,gc,domain,store_impl,tests}.rs (all ≤394 LOC)
- Delete store/types.rs (343 LOC) → split into types/{ids,events,outcomes,watermark,gc,errors,mod}.rs (all ≤80 LOC)

P1 — D4 single-writer fix:
- Remove #[allow(dead_code)] on kernel store field
- Add sig: String (serde default) to NostrEvent so relay events parse correctly
- ingest_timeline_event now routes through self.store.insert() with real relay URL

P2 — verify sigs (nostr crate wired):
- Add nostr = "0.44" dep to nmp-core
- VerifiedEvent newtype in store/types/events.rs: try_from_raw() calls nostr::Event::verify()
- from_raw_unchecked() escape hatch under cfg(any(test, feature = "test-support"))
- VerifyError in store/types/errors.rs (InvalidId, InvalidSignature, Serialization)
- EventStore::insert now takes VerifiedEvent — callers must verify before inserting
- kernel/ingest: replace "0".repeat(128) placeholder with VerifiedEvent::try_from_raw(); invalid-sig events logged + dropped

P2 — tombstone max-merge fix:
- merge_tombstone() takes max(deleted_at) and unions sources across re-deliveries

P2 — dup provenance fix:
- Exact-id duplicate checked BEFORE kind-specific supersession in handle_supersession()

P2 — D8 GC ceilings:
- claims field is BTreeSet<String> for idempotency
- gc::claim() enforces per-view ceiling (1000) and global pinned ceiling (20000)
- StoreError::OverPinned returned on breach

P3 — LMDB honesty: already compliant (all methods return Err(not_enabled()))

Bench gates (M1 live cold_start):
- first_item=294.81ms ≤ 800ms gate PASS (run 1779068929)
- first_item=383.81ms ≤ 800ms gate PASS (run 1779070804, post VerifiedEvent)

profile_thrashing live bench: snapshot_valid=false failure is pre-existing on origin/master
(confirmed by running against stashed kernel — same failure before any T29 changes)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>

=== T30 commit ===
bc84cfe fix(m2): codex follow-up — Rule 1 wildcard + plan-id stability + lane modeling + file splits + TODO removal (T30)
P1 — Rule 1 wildcard bug (lattice/mod.rs): empty kinds = wildcard; wildcard ∪
{1,6} now correctly returns wildcard (empty set), not {1,6}. Updated the
existing rule1_wildcard_absorbs_specific test (which codified the bug) and
added wildcard_unions_with_anything_stays_wildcard covering the negative-direction.

P1 — RoutingSource::Indexer removed (plan.rs): added UserConfiguredCategory
enum (AccountRead / AccountWrite / Indexer / Debug) so indexer fallback is
RoutingSource::UserConfigured(UserConfiguredCategory::Indexer) — lane 4, not a
fifth lane. Matches diagnostics.md §5.0 + ADR-0007 four-lane discipline. Updated
compiler and audit gate.

P1 — Plan-id binding fixed (compiler/plan_id.rs): hash now covers only
referenced pubkeys (authors ∪ addresses.pubkey ∪ #p tag values), not the
entire mailbox cache. Added CompileContext with indexer_set_version and
user_config_version. Sort all relay vectors before hashing. Include scope in
hash. Three new tests: plan_id_unchanged_when_unrelated_mailbox_arrives,
plan_id_changes_when_referenced_author_mailbox_updates,
plan_id_changes_on_indexer_set_version_bump (in m2_plan_id_stability.rs).

P2 — Direction table complete (compiler/partition.rs): added Case C (#p Inbox
direction with structural ban on non-inbox routes) and Case D (active-account
read relays for no-author hashtag firehose). IndexerProbe seam via
MailboxCache::request_probe() default no-op method.
SubscriptionCompiler::with_active_account_read_relays() constructor.

P2 — Audit strength: 300 → 1000 authors (Assertion 2). Assertion 5 now also
verifies merged address set is the union and both originating_interests tracked.

P2 — mod.rs minimized: submodules pub(crate); public surface narrowed to the
listed types. External audit test updated to import from planner:: re-exports.

P3 — File splits: compiler.rs (710 LOC) → compiler/{mod,mailbox,plan_id,partition}.rs
(191/105/159/258 LOC). lattice.rs (483 LOC) → lattice/{mod,rules}.rs (389/136 LOC).
m2_subscription_compilation_audit.rs → audit (460) + m2_plan_id_stability (225).

P3 — TODO removal: compiler.rs:183 TODO(phase2) → prose comment. interest.rs
TODO(nmp-nip19) → "Phase 2" prefix. plan.rs TODO(wire-emitter) → section header.
requests/{profile,thread}.rs TODO(M2-migration) → "M2 migration plan" headers.


=== T29 diff (cap 4000) ===
diff --git a/Cargo.lock b/Cargo.lock
index fd5485b..8120c2b 100644
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -2,6 +2,16 @@
 # It is not intended for manual editing.
 version = 4
 
+[[package]]
+name = "aead"
+version = "0.5.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "d122413f284cf2d62fb1b7db97e02edb8cda96d769b16e443a4f6195e35662b0"
+dependencies = [
+ "crypto-common",
+ "generic-array",
+]
+
 [[package]]
 name = "android_system_properties"
 version = "0.1.5"
@@ -11,12 +21,64 @@ dependencies = [
  "libc",
 ]
 
+[[package]]
+name = "arrayvec"
+version = "0.7.6"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "7c02d123df017efcdfbd739ef81735b36c5ba83ec3c59c80a9d7ecc718f92e50"
+
 [[package]]
 name = "autocfg"
 version = "1.5.0"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "c08606f8c3cbf4ce6ec8e28fb0014a2c086708fe954eaa885384a6165172e7e8"
 
+[[package]]
+name = "base64"
+version = "0.22.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "72b3254f16251a8381aa12e40e3c4d2f0199f8c6508fbecb9d91f575e0fbb8c6"
+
+[[package]]
+name = "base64ct"
+version = "1.8.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "2af50177e190e07a26ab74f8b1efbfe2ef87da2116221318cb1c2e82baf7de06"
+
+[[package]]
+name = "bech32"
+version = "0.11.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "32637268377fc7b10a8c6d51de3e7fba1ce5dd371a96e342b34e6078db558e7f"
+
+[[package]]
+name = "bip39"
+version = "2.2.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "90dbd31c98227229239363921e60fcf5e558e43ec69094d46fc4996f08d1d5bc"
+dependencies = [
+ "bitcoin_hashes",
+ "serde",
+ "unicode-normalization",
+]
+
+[[package]]
+name = "bitcoin-io"
+version = "0.1.4"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "2dee39a0ee5b4095224a0cfc6bf4cc1baf0f9624b96b367e53b66d974e51d953"
+
+[[package]]
+name = "bitcoin_hashes"
+version = "0.14.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "26ec84b80c482df901772e931a9a681e26a1b9ee2302edeff23cb30328745c8b"
+dependencies = [
+ "bitcoin-io",
+ "hex-conservative",
+ "serde",
+]
+
 [[package]]
 name = "block-buffer"
 version = "0.10.4"
@@ -26,6 +88,15 @@ dependencies = [
  "generic-array",
 ]
 
+[[package]]
+name = "block-padding"
+version = "0.3.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "a8894febbff9f758034a5b8e12d87918f56dfc64a8e1fe757d65e29041538d93"
+dependencies = [
+ "generic-array",
+]
+
 [[package]]
 name = "bumpalo"
 version = "3.20.2"
@@ -44,6 +115,15 @@ version = "1.11.1"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "1e748733b7cbc798e1434b6ac524f0c1ff2ab456fe201501e6497c8417a4fc33"
 
+[[package]]
+name = "cbc"
+version = "0.1.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "26b52a9543ae338f279b96b0b9fed9c8093744685043739079ce85cd58f289a6"
+dependencies = [
+ "cipher",
+]
+
 [[package]]
 name = "cc"
 version = "1.2.62"
@@ -60,6 +140,30 @@ version = "1.0.4"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "9330f8b2ff13f34540b44e946ef35111825727b38d33286ef986142615121801"
 
+[[package]]
+name = "chacha20"
+version = "0.9.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "c3613f74bd2eac03dad61bd53dbe620703d4371614fe0bc3b9f04dd36fe4e818"
+dependencies = [
+ "cfg-if",
+ "cipher",
+ "cpufeatures",
+]
+
+[[package]]
+name = "chacha20poly1305"
+version = "0.10.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "10cd79432192d1c0f4e1a0fef9527696cc039165d729fb41b3f4f4f354c2dc35"
+dependencies = [
+ "aead",
+ "chacha20",
+ "cipher",
+ "poly1305",
+ "zeroize",
+]
+
 [[package]]
 name = "chrono"
 version = "0.4.44"
@@ -71,6 +175,17 @@ dependencies = [
  "windows-link",
 ]
 
+[[package]]
+name = "cipher"
+version = "0.4.4"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "773f3b9af64447d2ce9850330c473515014aa235e6a783b02db81ff39e4a3dad"
+dependencies = [
+ "crypto-common",
+ "inout",
+ "zeroize",
+]
+
 [[package]]
 name = "core-foundation-sys"
 version = "0.8.7"
@@ -93,6 +208,7 @@ source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "78c8292055d1c1df0cce5d180393dc8cce0abec0a7102adb6c7b1eef6016d60a"
 dependencies = [
  "generic-array",
+ "rand_core",
  "typenum",
 ]
 
@@ -110,6 +226,18 @@ checksum = "9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292"
 dependencies = [
  "block-buffer",
  "crypto-common",
+ "subtle",
+]
+
+[[package]]
+name = "displaydoc"
+version = "0.2.5"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "97369cbbc041bc366949bc74d34658d6cda5621039731c6310521892a3a20ae0"
+dependencies = [
+ "proc-macro2",
+ "quote",
+ "syn",
 ]
 
 [[package]]
@@ -127,6 +255,15 @@ dependencies = [
  "serde_json",
 ]
 
+[[package]]
+name = "form_urlencoded"
+version = "1.2.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf"
+dependencies = [
+ "percent-encoding",
+]
+
 [[package]]
 name = "futures-core"
 version = "0.3.32"
@@ -168,8 +305,34 @@ source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "ff2abc00be7fca6ebc474524697ae276ad847ad0a6b3faa4bcb027e9a4614ad0"
 dependencies = [
  "cfg-if",
+ "js-sys",
  "libc",
  "wasi",
+ "wasm-bindgen",
+]
+
+[[package]]
+name = "hex"
+version = "0.4.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "7f24254aa9a54b5c858eaee2f5bccdb46aaf0e486a595ed5fd8f86ba55232a70"
+
+[[package]]
+name = "hex-conservative"
+version = "0.2.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "fda06d18ac606267c40c04e41b9947729bf8b9efe74bd4e82b61a5f26a510b9f"
+dependencies = [
+ "arrayvec",
+]
+
+[[package]]
+name = "hmac"
+version = "0.12.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "6c49c37c09c17a53d937dfbb742eb3a961d65a994e6bcdcf37e7399d0cc8ab5e"
+dependencies = [
+ "digest",
 ]
 
 [[package]]
@@ -212,6 +375,131 @@ dependencies = [
  "cc",
 ]
 
+[[package]]
+name = "icu_collections"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "2984d1cd16c883d7935b9e07e44071dca8d917fd52ecc02c04d5fa0b5a3f191c"
+dependencies = [
+ "displaydoc",
+ "potential_utf",
+ "utf8_iter",
+ "yoke",
+ "zerofrom",
+ "zerovec",
+]
+
+[[package]]
+name = "icu_locale_core"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "92219b62b3e2b4d88ac5119f8904c10f8f61bf7e95b640d25ba3075e6cac2c29"
+dependencies = [
+ "displaydoc",
+ "litemap",
+ "tinystr",
+ "writeable",
+ "zerovec",
+]
+
+[[package]]
+name = "icu_normalizer"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "c56e5ee99d6e3d33bd91c5d85458b6005a22140021cc324cea84dd0e72cff3b4"
+dependencies = [
+ "icu_collections",
+ "icu_normalizer_data",
+ "icu_properties",
+ "icu_provider",
+ "smallvec",
+ "zerovec",
+]
+
+[[package]]
+name = "icu_normalizer_data"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "da3be0ae77ea334f4da67c12f149704f19f81d1adf7c51cf482943e84a2bad38"
+
+[[package]]
+name = "icu_properties"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "bee3b67d0ea5c2cca5003417989af8996f8604e34fb9ddf96208a033901e70de"
+dependencies = [
+ "icu_collections",
+ "icu_locale_core",
+ "icu_properties_data",
+ "icu_provider",
+ "zerotrie",
+ "zerovec",
+]
+
+[[package]]
+name = "icu_properties_data"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "8e2bbb201e0c04f7b4b3e14382af113e17ba4f63e2c9d2ee626b720cbce54a14"
+
+[[package]]
+name = "icu_provider"
+version = "2.2.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "139c4cf31c8b5f33d7e199446eff9c1e02decfc2f0eec2c8d71f65befa45b421"
+dependencies = [
+ "displaydoc",
+ "icu_locale_core",
+ "writeable",
+ "yoke",
+ "zerofrom",
+ "zerotrie",
+ "zerovec",
+]
+
+[[package]]
+name = "idna"
+version = "1.1.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de"
+dependencies = [
+ "idna_adapter",
+ "smallvec",
+ "utf8_iter",
+]
+
+[[package]]
+name = "idna_adapter"
+version = "1.2.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "cb68373c0d6620ef8105e855e7745e18b0d00d3bdb07fb532e434244cdb9a714"
+dependencies = [
+ "icu_normalizer",
+ "icu_properties",
+]
+
+[[package]]
+name = "inout"
+version = "0.1.4"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "879f10e63c20629ecabbb64a8010319738c66a5cd0c29b02d63d272b03751d01"
+dependencies = [
+ "block-padding",
+ "generic-array",
+]
+
+[[package]]
+name = "instant"
+version = "0.1.13"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "e0242819d153cba4b4b05a5a8f2a7e9bbf97b6055b2a002b395c96b5ff3c0222"
+dependencies = [
+ "cfg-if",
+ "js-sys",
+ "wasm-bindgen",
+ "web-sys",
+]
+
 [[package]]
 name = "itoa"
 version = "1.0.18"
@@ -236,6 +524,12 @@ version = "0.2.186"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "68ab91017fe16c622486840e4c83c9a37afeff978bd239b5293d61ece587de66"
 
+[[package]]
+name = "litemap"
+version = "0.8.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "92daf443525c4cce67b150400bc2316076100ce0b3686209eb8cf3c31612e6f0"
+
 [[package]]
 name = "log"
 version = "0.4.29"
@@ -265,6 +559,7 @@ name = "nmp-core"
 version = "0.1.0"
 dependencies = [
  "chrono",
+ "nostr",
  "rustls",
  "serde",
  "serde_json",
@@ -281,6 +576,30 @@ dependencies = [
  "serde_json",
 ]
 
+[[package]]
+name = "nostr"
+version = "0.44.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "3aa5e3b6a278ed061835fe1ee293b71641e6bf8b401cfe4e1834bbf4ef0a34e1"
+dependencies = [
+ "base64",
+ "bech32",
+ "bip39",
+ "bitcoin_hashes",
+ "cbc",
+ "chacha20",
+ "chacha20poly1305",
+ "getrandom",
+ "hex",
+ "instant",
+ "scrypt",
+ "secp256k1",
+ "serde",
+ "serde_json",
+ "unicode-normalization",
+ "url",
+]
+
 [[package]]
 name = "num-traits"
 version = "0.2.19"
@@ -296,18 +615,71 @@ version = "1.21.4"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50"
 
+[[package]]
+name = "opaque-debug"
+version = "0.3.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "c08d65885ee38876c4f86fa503fb49d7b507c2b62552df7c70b2fce627e06381"
+
+[[package]]
+name = "password-hash"
+version = "0.5.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "346f04948ba92c43e8469c1ee6736c7563d71012b17d40745260fe106aac2166"
+dependencies = [
+ "base64ct",
+ "rand_core",
+ "subtle",
+]
+
 [[package]]
 name = "paste"
 version = "1.0.15"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "57c0d7b74b563b49d38dae00a0c37d4d6de9b432382b2892f0574ddcae73fd0a"
 
+[[package]]
+name = "pbkdf2"
+version = "0.12.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "f8ed6a7761f76e3b9f92dfb0a60a6a6477c61024b775147ff0973a02653abaf2"
+dependencies = [
+ "digest",
+ "hmac",
+]
+
+[[package]]
+name = "percent-encoding"
+version = "2.3.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220"
+
 [[package]]
 name = "pin-project-lite"
 version = "0.2.17"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "a89322df9ebe1c1578d689c92318e070967d1042b512afbe49518723f4e6d5cd"
 
+[[package]]
+name = "poly1305"
+version = "0.8.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "8159bd90725d2df49889a078b54f4f79e87f1f8a8444194cdca81d38f5393abf"
+dependencies = [
+ "cpufeatures",
+ "opaque-debug",
+ "universal-hash",
+]
+
+[[package]]
+name = "potential_utf"
+version = "0.1.5"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "0103b1cef7ec0cf76490e969665504990193874ea05c85ff9bab8b911d0a0564"
+dependencies = [
+ "zerovec",
+]
+
 [[package]]
 name = "ppv-lite86"
 version = "0.2.21"
@@ -419,6 +791,47 @@ version = "1.0.22"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "b39cdef0fa800fc44525c84ccb54a029961a8215f9619753635a9c0d2538d46d"
 
+[[package]]
+name = "salsa20"
+version = "0.10.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "97a22f5af31f73a954c10289c93e8a50cc23d971e80ee446f1f6f7137a088213"
+dependencies = [
+ "cipher",
+]
+
+[[package]]
+name = "scrypt"
+version = "0.11.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "0516a385866c09368f0b5bcd1caff3366aace790fcd46e2bb032697bb172fd1f"
+dependencies = [
+ "password-hash",
+ "pbkdf2",
+ "salsa20",
+ "sha2",
+]
+
+[[package]]
+name = "secp256k1"
+version = "0.29.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "9465315bc9d4566e1724f0fffcbcc446268cb522e60f9a27bcded6b19c108113"
+dependencies = [
+ "rand",
+ "secp256k1-sys",
+ "serde",
+]
+
+[[package]]
+name = "secp256k1-sys"
+version = "0.10.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "d4387882333d3aa8cb20530a17c69a3752e97837832f34f6dccc760e715001d9"
+dependencies = [
+ "cc",
+]
+
 [[package]]
 name = "serde"
 version = "1.0.228"
@@ -473,6 +886,17 @@ dependencies = [
  "digest",
 ]
 
+[[package]]
+name = "sha2"
+version = "0.10.9"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283"
+dependencies = [
+ "cfg-if",
+ "cpufeatures",
+ "digest",
+]
+
 [[package]]
 name = "shlex"
 version = "1.3.0"
@@ -485,6 +909,18 @@ version = "0.4.12"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "0c790de23124f9ab44544d7ac05d60440adc586479ce501c1d6d7da3cd8c9cf5"
 
+[[package]]
+name = "smallvec"
+version = "1.15.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "67b1b7a3b5fe4f1376887184045fcf45c69e92af734b7aaddc05fb777b6fbd03"
+
+[[package]]
+name = "stable_deref_trait"
+version = "1.2.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596"
+
 [[package]]
 name = "subtle"
 version = "2.6.1"
@@ -502,6 +938,17 @@ dependencies = [
  "unicode-ident",
 ]
 
+[[package]]
+name = "synstructure"
+version = "0.13.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "728a70f3dbaf5bab7f0c4b1ac8d7ae5ea60a4b5549c8a5914361c99147a709d2"
+dependencies = [
+ "proc-macro2",
+ "quote",
+ "syn",
+]
+
 [[package]]
 name = "thiserror"
 version = "1.0.69"
@@ -522,6 +969,31 @@ dependencies = [
  "syn",
 ]
 
+[[package]]
+name = "tinystr"
+version = "0.8.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "c8323304221c2a851516f22236c5722a72eaa19749016521d6dff0824447d96d"
+dependencies = [
+ "displaydoc",
+ "zerovec",
+]
+
+[[package]]
+name = "tinyvec"
+version = "1.11.0"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "3e61e67053d25a4e82c844e8424039d9745781b3fc4f32b8d55ed50f5f667ef3"
+dependencies = [
+ "tinyvec_macros",
+]
+
+[[package]]
+name = "tinyvec_macros"
+version = "0.1.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "1f3ccbac311fea05f86f61904b462b55fb3df8837a366dfc601a0161d0532f20"
+
 [[package]]
 name = "tungstenite"
 version = "0.24.0"
@@ -555,18 +1027,56 @@ version = "1.0.24"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75"
 
+[[package]]
+name = "unicode-normalization"
+version = "0.1.25"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8"
+dependencies = [
+ "tinyvec",
+]
+
+[[package]]
+name = "universal-hash"
+version = "0.5.1"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "fc1de2c688dc15305988b563c3854064043356019f97a4b46276fe734c4f07ea"
+dependencies = [
+ "crypto-common",
+ "subtle",
+]
+
 [[package]]
 name = "untrusted"
 version = "0.9.0"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1"
 
+[[package]]
+name = "url"
+version = "2.5.8"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed"
+dependencies = [
+ "form_urlencoded",
+ "idna",
+ "percent-encoding",
+ "serde",
+ "serde_derive",
+]
+
 [[package]]
 name = "utf-8"
 version = "0.7.6"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "09cc8ee72d2a9becf2f2febe0205bbed8fc6615b7cb429ad062dc7b7ddd036a9"
 
+[[package]]
+name = "utf8_iter"
+version = "1.0.4"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be"
+
 [[package]]
 name = "version_check"
 version = "0.9.5"
@@ -624,6 +1134,16 @@ dependencies = [
  "unicode-ident",
 ]
 
+[[package]]
+name = "web-sys"
+version = "0.3.98"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "4b572dff8bcf38bad0fa19729c89bb5748b2b9b1d8be70cf90df697e3a8f32aa"
+dependencies = [
+ "js-sys",
+ "wasm-bindgen",
+]
+
 [[package]]
 name = "webpki-roots"
 version = "0.26.11"
@@ -774,6 +1294,35 @@ version = "0.52.6"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "589f6da84c646204747d1270a2a5661ea66ed1cced2631d546fdfb155959f9ec"
 
+[[package]]
+name = "writeable"
+version = "0.6.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "1ffae5123b2d3fc086436f8834ae3ab053a283cfac8fe0a0b8eaae044768a4c4"
+
+[[package]]
+name = "yoke"
+version = "0.8.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "abe8c5fda708d9ca3df187cae8bfb9ceda00dd96231bed36e445a1a48e66f9ca"
+dependencies = [
+ "stable_deref_trait",
+ "yoke-derive",
+ "zerofrom",
+]
+
+[[package]]
+name = "yoke-derive"
+version = "0.8.2"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "de844c262c8848816172cef550288e7dc6c7b7814b4ee56b3e1553f275f1858e"
+dependencies = [
+ "proc-macro2",
+ "quote",
+ "syn",
+ "synstructure",
+]
+
 [[package]]
 name = "zerocopy"
 version = "0.8.48"
@@ -794,12 +1343,66 @@ dependencies = [
  "syn",
 ]
 
+[[package]]
+name = "zerofrom"
+version = "0.1.8"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "0ec05a11813ea801ff6d75110ad09cd0824ddba17dfe17128ea0d5f68e6c5272"
+dependencies = [
+ "zerofrom-derive",
+]
+
+[[package]]
+name = "zerofrom-derive"
+version = "0.1.7"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "11532158c46691caf0f2593ea8358fed6bbf68a0315e80aae9bd41fbade684a1"
+dependencies = [
+ "proc-macro2",
+ "quote",
+ "syn",
+ "synstructure",
+]
+
 [[package]]
 name = "zeroize"
 version = "1.8.2"
 source = "registry+https://github.com/rust-lang/crates.io-index"
 checksum = "b97154e67e32c85465826e8bcc1c59429aaaf107c1e4a9e53c8d8ccd5eff88d0"
 
+[[package]]
+name = "zerotrie"
+version = "0.2.4"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "0f9152d31db0792fa83f70fb2f83148effb5c1f5b8c7686c3459e361d9bc20bf"
+dependencies = [
+ "displaydoc",
+ "yoke",
+ "zerofrom",
+]
+
+[[package]]
+name = "zerovec"
+version = "0.11.6"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "90f911cbc359ab6af17377d242225f4d75119aec87ea711a880987b18cd7b239"
+dependencies = [
+ "yoke",
+ "zerofrom",
+ "zerovec-derive",
+]
+
+[[package]]
+name = "zerovec-derive"
+version = "0.11.3"
+source = "registry+https://github.com/rust-lang/crates.io-index"
+checksum = "625dc425cab0dca6dc3c3319506e6593dcb08a9f387ea3b284dbd52a92c40555"
+dependencies = [
+ "proc-macro2",
+ "quote",
+ "syn",
+]
+
 [[package]]
 name = "zmij"
 version = "1.0.21"
diff --git a/crates/nmp-core/Cargo.toml b/crates/nmp-core/Cargo.toml
index 59b77dd..49fbbd7 100644
--- a/crates/nmp-core/Cargo.toml
+++ b/crates/nmp-core/Cargo.toml
@@ -17,6 +17,7 @@ lmdb-backend = []
 
 [dependencies]
 chrono = { version = "0.4", default-features = false, features = ["clock"] }
+nostr = "0.44"
 rustls = { version = "0.23", default-features = false, features = ["ring"] }
 serde = { version = "1.0", features = ["derive"] }
 serde_json = "1.0"
diff --git a/crates/nmp-core/src/kernel/ingest.rs b/crates/nmp-core/src/kernel/ingest.rs
index 35fcd12..31037bd 100644
--- a/crates/nmp-core/src/kernel/ingest.rs
+++ b/crates/nmp-core/src/kernel/ingest.rs
@@ -155,7 +155,7 @@ impl Kernel {
 
         match event.kind {
             0 => self.ingest_profile(event),
-            1 | 6 => self.ingest_timeline_event(sub_id, event),
+            1 | 6 => self.ingest_timeline_event(role, sub_id, event),
             3 => self.ingest_contacts(event),
             10002 => self.ingest_relay_list(event),
             _ => {}
@@ -232,10 +232,11 @@ impl Kernel {
         }
     }
 
-    pub(super) fn ingest_timeline_event(&mut self, sub_id: &str, event: NostrEvent) {
+    pub(super) fn ingest_timeline_event(&mut self, role: RelayRole, sub_id: &str, event: NostrEvent) {
+        // Duplicate check on the in-memory read-cache.
         if self.events.contains_key(&event.id) {
-            if let Some(stored) = self.events.get_mut(&event.id) {
-                stored.relay_count = stored.relay_count.saturating_add(1);
+            if let Some(cached) = self.events.get_mut(&event.id) {
+                cached.relay_count = cached.relay_count.saturating_add(1);
             }
             return;
         }
@@ -244,7 +245,54 @@ impl Kernel {
             return;
         }
 
-        let stored = StoredEvent {
+        // D4: route through EventStore (the single writer).
+        // Signature verification via VerifiedEvent::try_from_raw. Events that
+        // fail verification are logged and dropped — not cached locally.
+        let raw = crate::store::RawEvent {
+            id: event.id.clone(),
+            pubkey: event.pubkey.clone(),
+            created_at: event.created_at,
+            kind: event.kind,
+            tags: event.tags.clone(),
+            content: event.content.clone(),
+            sig: event.sig.clone(),
+        };
+        let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
+            Ok(v) => v,
+            Err(e) => {
+                self.log(format!("sig verify failed for {}: {e}", &event.id[..16]));
+                return;
+            }
+        };
+        let relay_url = role.url().to_string();
+        let received_at_ms = std::time::SystemTime::now()
+            .duration_since(std::time::UNIX_EPOCH)
+            .map(|d| d.as_millis() as u64)
+            .unwrap_or(0);
+        // Store insert; log but don't abort on error (graceful degradation).
+        match self.store.insert(verified, &relay_url, received_at_ms) {
+            Ok(outcome) => {
+                use crate::store::InsertOutcome;
+                match outcome {
+                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => {}
+                    InsertOutcome::Duplicate { .. } | InsertOutcome::Superseded { .. } => {
+                        // Store already has a valid version; still cache locally for timeline.
+                    }
+                    InsertOutcome::Tombstoned { .. } | InsertOutcome::Rejected { .. }
+                    | InsertOutcome::Ephemeral { .. } => {
+                        // Store rejected the event; skip populating local cache.
+                        return;
+                    }
+                }
+            }
+            Err(e) => {
+                self.log(format!("store insert error: {e}"));
+                // Graceful degradation: continue with local-cache-only path.
+            }
+        }
+
+        // Populate the lightweight read-cache for timeline ordering + display.
+        let cached = StoredEvent {
             id: event.id.clone(),
             author: event.pubkey.clone(),
             kind: event.kind,
@@ -253,7 +301,7 @@ impl Kernel {
             content: event.content,
             relay_count: 1,
         };
-        self.events.insert(event.id.clone(), stored);
+        self.events.insert(event.id.clone(), cached);
         if sub_id.starts_with("diag-firehose-") {
             self.diagnostic_firehose_events = self.diagnostic_firehose_events.saturating_add(1);
         }
diff --git a/crates/nmp-core/src/kernel/mod.rs b/crates/nmp-core/src/kernel/mod.rs
index d4f3835..5f425f5 100644
--- a/crates/nmp-core/src/kernel/mod.rs
+++ b/crates/nmp-core/src/kernel/mod.rs
@@ -283,13 +283,12 @@ struct ViewInterest {
 }
 
 pub(crate) struct Kernel {
-    /// Pluggable event store. Defaults to `MemEventStore`; will be replaced by
-    /// `LmdbEventStore` once the full M3 LMDB integration is complete.
+    /// Pluggable event store. D4: the single writer for all Nostr events.
     ///
-    /// The existing `events: HashMap<String, StoredEvent>` field is preserved
-    /// for backward compatibility during the M3 migration. The store field is
-    /// the target home for all event persistence after M3 completes.
-    #[allow(dead_code)]
+    /// `MemEventStore` by default; replace with `LmdbEventStore` in M3 phase 2.
+    /// `ingest_timeline_event` routes every new event through `store.insert()`.
+    /// `events: HashMap<String, kernel::StoredEvent>` is kept as a lightweight
+    /// read-cache (timeline ordering + content display) derived from store outcomes.
     store: Box<dyn EventStore>,
     rev: u64,
     visible_limit: usize,
diff --git a/crates/nmp-core/src/kernel/nostr.rs b/crates/nmp-core/src/kernel/nostr.rs
index 64d2ad9..10df222 100644
--- a/crates/nmp-core/src/kernel/nostr.rs
+++ b/crates/nmp-core/src/kernel/nostr.rs
@@ -8,6 +8,10 @@ pub(super) struct NostrEvent {
     pub(super) kind: u32,
     pub(super) tags: Vec<Vec<String>>,
     pub(super) content: String,
+    /// Schnorr signature (hex). Present in all valid NIP-01 events.
+    /// Default to empty string so legacy test fixtures without `sig` still parse.
+    #[serde(default)]
+    pub(super) sig: String,
 }
 
 #[derive(Default, Deserialize)]
diff --git a/crates/nmp-core/src/store/events.rs b/crates/nmp-core/src/store/events.rs
index 822fa7b..4408da0 100644
--- a/crates/nmp-core/src/store/events.rs
+++ b/crates/nmp-core/src/store/events.rs
@@ -8,8 +8,8 @@ use std::sync::{Arc, Mutex};
 
 use super::types::{
     ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
-    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RelayUrl, StoredEvent, TombstoneRow,
-    WatermarkKey, WatermarkRow,
+    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoredEvent, TombstoneRow,
+    VerifiedEvent, WatermarkKey, WatermarkRow,
 };
 use super::StoreError;
 use crate::substrate::DomainMigration;
@@ -231,10 +231,11 @@ pub trait EventStore: Send + Sync {
     /// updates secondaries + provenance + tombstones atomically.
     /// Returns `InsertOutcome` per §7.1.
     ///
-    /// NOTE: uses `RawEvent` instead of `nostr::Event` until the nostr crate is wired in.
+    /// Callers must verify the event before calling this method; `VerifiedEvent`
+    /// is the proof-of-verification token.
     fn insert(
         &self,
-        event: RawEvent,
+        event: VerifiedEvent,
         source: &RelayUrl,
         received_at_ms: u64,
     ) -> Result<InsertOutcome, StoreError>;
diff --git a/crates/nmp-core/src/store/lmdb.rs b/crates/nmp-core/src/store/lmdb.rs
index 224e380..4cff28a 100644
--- a/crates/nmp-core/src/store/lmdb.rs
+++ b/crates/nmp-core/src/store/lmdb.rs
@@ -12,8 +12,8 @@ use std::path::{Path, PathBuf};
 use super::events::{DomainHandle, EventIter, EventStore};
 use super::types::{
     ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
-    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RelayUrl, StoredEvent, TombstoneRow,
-    WatermarkKey, WatermarkRow,
+    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoredEvent, TombstoneRow,
+    VerifiedEvent, WatermarkKey, WatermarkRow,
 };
 use super::StoreError;
 use crate::substrate::DomainMigration;
@@ -148,7 +148,7 @@ impl EventStore for LmdbEventStore {
 
     fn insert(
         &self,
-        _event: RawEvent,
+        _event: VerifiedEvent,
         _source: &RelayUrl,
         _received_at_ms: u64,
     ) -> Result<InsertOutcome, StoreError> {
diff --git a/crates/nmp-core/src/store/mem.rs b/crates/nmp-core/src/store/mem.rs
deleted file mode 100644
index 0d4db74..0000000
--- a/crates/nmp-core/src/store/mem.rs
+++ /dev/null
@@ -1,1105 +0,0 @@
-//! In-memory `EventStore` backend.
-//!
-//! Used for tests and the pre-M15 web target. Every method is fully implemented
-//! against a `Mutex<MemState>` so tests cover the same logic that the LMDB
-//! backend will eventually call.
-//!
-//! See `docs/design/lmdb/trait.md` §5 ("Two backends in v1").
-
-use std::collections::HashMap;
-use std::sync::{Arc, Mutex};
-
-/// Shared storage map for a single domain namespace.
-type DomainMap = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;
-
-use super::events::{DomainHandle, DomainHandleInner, EventIter, EventStore};
-use super::types::{
-    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
-    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RejectReason, RelayUrl, StoredEvent,
-    TombstoneOrigin, TombstoneRow, WatermarkKey, WatermarkRow,
-};
-use super::StoreError;
-use crate::substrate::DomainMigration;
-
-// ─── Constants ────────────────────────────────────────────────────────────────
-
-/// Default maximum pinned events per view.
-const DEFAULT_VIEW_CEILING: usize = 1_000;
-
-/// Maximum provenance entries kept per event.
-const MAX_PROVENANCE_ENTRIES: usize = 32;
-
-/// Tombstones older than this many seconds are purged by `gc_step`.
-const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600; // 90 days
-
-// ─── Inner state ─────────────────────────────────────────────────────────────
-
-struct MemState {
-    /// Primary event store: hex id → StoredEvent.
-    events: HashMap<String, StoredEvent>,
-
-    /// Tombstone rows: hex target_id → TombstoneRow.
-    tombstones: HashMap<String, TombstoneRow>,
-
-    /// Address tombstones (kind:5 `a`-tag): "kind:pubkey:dtag" → TombstoneRow.
-    addr_tombstones: HashMap<String, TombstoneRow>,
-
-    /// Provenance: hex event_id → sorted Vec<ProvenanceEntry>.
-    provenance: HashMap<String, Vec<ProvenanceEntry>>,
-
-    /// Watermarks: (filter_hash_hex, relay_url) → WatermarkRow.
-    watermarks: HashMap<(String, String), WatermarkRow>,
-
-    /// Domain data per namespace.
-    domain_data: HashMap<&'static str, DomainMap>,
-
-    /// Domain schema versions.
-    domain_versions: HashMap<&'static str, u32>,
-
-    /// Claim budgets: claimer → max pinned.
-    claim_budgets: HashMap<ClaimerId, usize>,
-
-    /// Current claims: claimer → set of hex event ids.
-    claims: HashMap<ClaimerId, Vec<String>>,
-}
-
-impl MemState {
-    fn new() -> Self {
-        Self {
-            events: HashMap::new(),
-            tombstones: HashMap::new(),
-            addr_tombstones: HashMap::new(),
-            provenance: HashMap::new(),
-            watermarks: HashMap::new(),
-            domain_data: HashMap::new(),
-            domain_versions: HashMap::new(),
-            claim_budgets: HashMap::new(),
-            claims: HashMap::new(),
-        }
-    }
-
-    #[allow(dead_code)] // Available for future dump/debug helpers.
-    fn events_sorted_newest_first(&self) -> Vec<&StoredEvent> {
-        let mut v: Vec<&StoredEvent> = self.events.values().collect();
-        v.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at)
-                .then(a.raw.id.cmp(&b.raw.id))
-        });
-        v
-    }
-}
-
-// ─── MemEventStore ────────────────────────────────────────────────────────────
-
-/// Fully in-memory `EventStore` implementation.
-pub struct MemEventStore {
-    state: Mutex<MemState>,
-}
-
-impl MemEventStore {
-    pub fn new() -> Self {
-        Self {
-            state: Mutex::new(MemState::new()),
-        }
-    }
-
-    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemState>, StoreError> {
-        self.state.lock().map_err(|e| StoreError::Io(e.to_string()))
-    }
-}
-
-impl Default for MemEventStore {
-    fn default() -> Self {
-        Self::new()
-    }
-}
-
-// ─── Provenance helpers ───────────────────────────────────────────────────────
-
-fn sort_provenance(entries: &mut [ProvenanceEntry]) {
-    entries.sort_by(|a, b| {
-        a.first_seen_ms.cmp(&b.first_seen_ms)
-            .then(a.relay_url.cmp(&b.relay_url))
-    });
-    for (i, e) in entries.iter_mut().enumerate() {
-        e.primary = i == 0;
-    }
-}
-
-fn upsert_provenance(
-    entries: &mut Vec<ProvenanceEntry>,
-    relay_url: RelayUrl,
-    received_at_ms: u64,
-) {
-    // Update existing entry if present.
-    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
-        if received_at_ms < e.first_seen_ms {
-            e.first_seen_ms = received_at_ms;
-        }
-        if received_at_ms > e.last_seen_ms {
-            e.last_seen_ms = received_at_ms;
-        }
-        sort_provenance(entries);
-        return;
-    }
-
-    // If at capacity, overwrite the oldest non-primary entry.
-    if entries.len() >= MAX_PROVENANCE_ENTRIES {
-        // Primary is entries[0] after sort; replace oldest non-primary by last_seen_ms.
-        if let Some(oldest) = entries.iter_mut().skip(1)
-            .min_by_key(|e| e.last_seen_ms)
-        {
-            *oldest = ProvenanceEntry {
-                relay_url,
-                first_seen_ms: received_at_ms,
-                last_seen_ms: received_at_ms,
-                primary: false,
-            };
-            sort_provenance(entries);
-            return;
-        }
-    }
-
-    entries.push(ProvenanceEntry {
-        relay_url,
-        first_seen_ms: received_at_ms,
-        last_seen_ms: received_at_ms,
-        primary: false,
-    });
-    sort_provenance(entries);
-}
-
-// ─── EventStore impl ─────────────────────────────────────────────────────────
-
-impl EventStore for MemEventStore {
-    // ─── Reads ───────────────────────────────────────────────────────────────
-
-    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
-        let hex = bytes_to_hex(id);
-        let st = self.lock()?;
-        Ok(st.events.get(&hex).cloned())
-    }
-
-    fn scan_by_author_kind<'a>(
-        &'a self,
-        author: &PubKey,
-        kinds: &[u32],
-        since: Option<u64>,
-        until: Option<u64>,
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let author_hex = bytes_to_hex(author);
-        let st = self.lock()?;
-        let mut results: Vec<StoredEvent> = st.events.values()
-            .filter(|ev| {
-                ev.raw.pubkey == author_hex
-                    && kinds.contains(&ev.raw.kind)
-                    && since.is_none_or(|s| ev.raw.created_at >= s)
-                    && until.is_none_or(|u| ev.raw.created_at <= u)
-            })
-            .cloned()
-            .collect();
-        results.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
-        });
-        results.truncate(limit);
-        Ok(Box::new(results.into_iter().map(Ok)))
-    }
-
-    fn get_param_replaceable(
-        &self,
-        pubkey: &PubKey,
-        kind: u32,
-        d_tag: &[u8],
-    ) -> Result<Option<StoredEvent>, StoreError> {
-        let pubkey_hex = bytes_to_hex(pubkey);
-        let d_str = String::from_utf8_lossy(d_tag).into_owned();
-        let st = self.lock()?;
-        let winner = st.events.values()
-            .filter(|ev| {
-                ev.raw.pubkey == pubkey_hex
-                    && ev.raw.kind == kind
-                    && ev.raw.d_tag()
-                        .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
-                        .unwrap_or(false)
-            })
-            .max_by(|a, b| {
-                a.raw.created_at.cmp(&b.raw.created_at)
-                    .then(b.raw.id.cmp(&a.raw.id))
-            })
-            .cloned();
-        Ok(winner)
-    }
-
-    fn scan_by_kind_dtag<'a>(
-        &'a self,
-        kind: u32,
-        d_tag: &[u8],
-        since: Option<u64>,
-        until: Option<u64>,
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let d_str = String::from_utf8_lossy(d_tag).into_owned();
-        let st = self.lock()?;
-        let mut results: Vec<StoredEvent> = st.events.values()
-            .filter(|ev| {
-                ev.raw.kind == kind
-                    && ev.raw.d_tag()
-                        .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
-                        .unwrap_or(false)
-                    && since.is_none_or(|s| ev.raw.created_at >= s)
-                    && until.is_none_or(|u| ev.raw.created_at <= u)
-            })
-            .cloned()
-            .collect();
-        results.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
-        });
-        results.truncate(limit);
-        Ok(Box::new(results.into_iter().map(Ok)))
-    }
-
-    fn scan_by_etag<'a>(
-        &'a self,
-        target: &EventId,
-        kinds: &[u32],
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let target_hex = bytes_to_hex(target);
-        let st = self.lock()?;
-        let mut results: Vec<StoredEvent> = st.events.values()
-            .filter(|ev| {
-                kinds.contains(&ev.raw.kind)
-                    && ev.raw.e_tags().contains(&target_hex)
-            })
-            .cloned()
-            .collect();
-        results.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
-        });
-        results.truncate(limit);
-        Ok(Box::new(results.into_iter().map(Ok)))
-    }
-
-    fn scan_by_ptag<'a>(
-        &'a self,
-        target: &PubKey,
-        kinds: &[u32],
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let target_hex = bytes_to_hex(target);
-        let st = self.lock()?;
-        let mut results: Vec<StoredEvent> = st.events.values()
-            .filter(|ev| {
-                kinds.contains(&ev.raw.kind)
-                    && ev.raw.p_tags().contains(&target_hex)
-            })
-            .cloned()
-            .collect();
-        results.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
-        });
-        results.truncate(limit);
-        Ok(Box::new(results.into_iter().map(Ok)))
-    }
-
-    fn scan_by_kind_time<'a>(
-        &'a self,
-        kinds: &[u32],
-        since: Option<u64>,
-        until: Option<u64>,
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let st = self.lock()?;
-        let mut results: Vec<StoredEvent> = st.events.values()
-            .filter(|ev| {
-                (kinds.is_empty() || kinds.contains(&ev.raw.kind))
-                    && since.is_none_or(|s| ev.raw.created_at >= s)
-                    && until.is_none_or(|u| ev.raw.created_at <= u)
-            })
-            .cloned()
-            .collect();
-        results.sort_by(|a, b| {
-            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
-        });
-        results.truncate(limit);
-        Ok(Box::new(results.into_iter().map(Ok)))
-    }
-
-    fn scan_expiring_before<'a>(
-        &'a self,
-        unix_seconds: u64,
-        limit: usize,
-    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
-        let st = self.lock()?;
-        // Ascending by expiration.
-        let mut pairs: Vec<(u64, StoredEvent)> = st.events.values()
-            .filter_map(|ev| {
-                ev.raw.expiration()
-                    .filter(|&exp| exp < unix_seconds)
-                    .map(|exp| (exp, ev.clone()))
-            })
-            .collect();
-        pairs.sort_by_key(|(exp, _)| *exp);
-        pairs.truncate(limit);
-        Ok(Box::new(pairs.into_iter().map(|(_, ev)| Ok(ev))))
-    }
-
-    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
-        let hex = bytes_to_hex(target);
-        let st = self.lock()?;
-        Ok(st.tombstones.get(&hex).cloned().into_iter().collect())
-    }
-
-    fn list_tombstones<'a>(
-        &'a self,
-    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
-    {
-        let st = self.lock()?;
-        let rows: Vec<TombstoneRow> = st.tombstones.values().cloned().collect();
-        Ok(Box::new(rows.into_iter().map(Ok)))
-    }
-
-    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
-        let hex = bytes_to_hex(id);
-        let st = self.lock()?;
-        Ok(st.provenance.get(&hex).cloned().unwrap_or_default())
-    }
-
-    // ─── Writes ──────────────────────────────────────────────────────────────
-
-    fn insert(
-        &self,
-        event: RawEvent,
-        source: &RelayUrl,
-        received_at_ms: u64,
-    ) -> Result<InsertOutcome, StoreError> {
-        // 1. Structural validation (sig check deferred to nostr crate wiring).
-        if !event.is_structurally_valid() {
-            return Ok(InsertOutcome::Rejected {
-                id: event.id_bytes(),
-                reason: RejectReason::Malformed("invalid id/pubkey/sig length".into()),
-            });
-        }
-
-        // 2. Ephemeral: deliver to live consumers, do not store.
-        if event.is_ephemeral() {
-            return Ok(InsertOutcome::Ephemeral { id: event.id_bytes() });
-        }
-
-        // 3. Check NIP-40 expiration on arrival.
-        if let Some(exp) = event.expiration() {
-            let now_secs = received_at_ms / 1000;
-            if exp <= now_secs {
-                return Ok(InsertOutcome::Rejected {
-                    id: event.id_bytes(),
-                    reason: RejectReason::ExpiredOnArrival,
-                });
-            }
-        }
-
-        let id_bytes = event.id_bytes();
-        let id_hex = event.id.clone();
-        let mut st = self.lock()?;
-
-        // 4. Check tombstone (per-id).
-        //
-        // For Kind5 tombstones created pre-emptively (target arrived after kind:5), enforce
-        // the self-delete invariant: only suppress if the tombstone's deleter owns this event.
-        // Foreign kind:5 tombstones (deleter != event.pubkey) MUST NOT block the rightful event.
-        // When a foreign pre-tombstone is found to not apply, remove it to maintain invariant 3
-        // (no tombstone whose target is in the primary store).
-        if let Some(tomb) = st.tombstones.get(&id_hex).cloned() {
-            let tombstone_applies = match tomb.origin {
-                TombstoneOrigin::Kind5 => {
-                    // Self-delete check: the deleter must be the event author.
-                    tomb.deleter_pubkey
-                        .as_ref()
-                        .map(|dp| bytes_to_hex(dp) == event.pubkey)
-                        .unwrap_or(false)
-                }
-                // NIP-40 expiry and admin purge tombstones always apply.
-                TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
-            };
-            if tombstone_applies {
-                return Ok(InsertOutcome::Tombstoned {
-                    id: id_bytes,
-                    kind5_event_id: tomb.kind5_event_id,
-                    origin: tomb.origin,
-                });
-            }
-            // Foreign pre-tombstone does not apply — remove it so the event can be inserted
-            // and invariant 3 (no tombstone with live primary target) is maintained.
-            st.tombstones.remove(&id_hex);
-        }
-
-        // 5. Check address tombstone for parameterized replaceables.
-        if event.is_param_replaceable() {
-            if let Some(d) = event.d_tag() {
-                let d_str = String::from_utf8_lossy(&d).into_owned();
-                let addr_key = format!("{}:{}:{}", event.kind, event.pubkey, d_str);
-                if let Some(tomb) = st.addr_tombstones.get(&addr_key) {
-                    // Only suppress if the kind:5 is newer than (or equal to) this event.
-                    if tomb.deleted_at >= event.created_at {
-                        return Ok(InsertOutcome::Tombstoned {
-                            id: id_bytes,
-                            kind5_event_id: tomb.kind5_event_id,
-                            origin: tomb.origin,
-                        });
-                    }
-                }
-            }
-        }
-
-        // 6. Kind:5 handling (self-deletes only — foreign kind:5 is stored but ignored).
-        if event.kind == 5 {
-            return handle_kind5_insert(&mut st, event, source, received_at_ms);
-        }
-
-        // 7. Replaceable supersession.
-        if event.is_replaceable() {
-            return handle_replaceable_insert(&mut st, event, source, received_at_ms);
-        }
-
-        // 8. Parameterized replaceable.
-        if event.is_param_replaceable() {
-            return handle_param_replaceable_insert(&mut st, event, source, received_at_ms);
-        }
-
-        // 9. Normal insert / duplicate.
-        handle_normal_insert(&mut st, event, source, received_at_ms)
-    }
-
-    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
-        let mut st = self.lock()?;
-        let ids_to_remove: Vec<String> = match &filter {
-            DeleteFilter::ByRelayOnly(relay) => {
-                // Remove events where the only provenance source is this relay.
-                st.events.keys()
-                    .filter(|id| {
-                        st.provenance.get(*id)
-                            .map(|p| p.len() == 1 && p[0].relay_url == *relay)
-                            .unwrap_or(false)
-                    })
-                    .cloned()
-                    .collect()
-            }
-            DeleteFilter::ByAuthor(pk) => {
-                let pk_hex = bytes_to_hex(pk);
-                st.events.iter()
-                    .filter(|(_, ev)| ev.raw.pubkey == pk_hex)
-                    .map(|(id, _)| id.clone())
-                    .collect()
-            }
-            DeleteFilter::ByIds(ids) => {
-                ids.iter().map(|id| bytes_to_hex(id)).filter(|h| st.events.contains_key(h)).collect()
-            }
-            DeleteFilter::ByKindRange { lo, hi } => {
-                st.events.iter()
-                    .filter(|(_, ev)| ev.raw.kind >= *lo && ev.raw.kind <= *hi)
-                    .map(|(id, _)| id.clone())
-                    .collect()
-            }
-        };
-        let count = ids_to_remove.len();
-        for id in ids_to_remove {
-            st.events.remove(&id);
-            st.provenance.remove(&id);
-        }
-        Ok(count)
-    }
-
-    // ─── Watermarks ──────────────────────────────────────────────────────────
-
-    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
-        let st = self.lock()?;
-        let wm_key = (bytes_to_hex(&key.filter_hash), key.relay_url.clone());
-        Ok(st.watermarks.get(&wm_key).cloned())
-    }
-
-    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError> {
-        let mut st = self.lock()?;
-        let wm_key = (bytes_to_hex(&row.key.filter_hash), row.key.relay_url.clone());
-        st.watermarks.insert(wm_key, row);
-        Ok(())
-    }
-
-    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError> {
-        let row = self.read_watermark(key)?;
-        let Some(row) = row else {
-            return Ok(Coverage::Unknown);
-        };
-        // Default staleness window: 300 seconds.
-        let staleness_window = 300u64;
-        let now = std::time::SystemTime::now()
-            .duration_since(std::time::UNIX_EPOCH)
-            .map(|d| d.as_secs())
-            .unwrap_or(0);
-        let age = now.saturating_sub(row.updated_at);
-        if age <= staleness_window {
-            Ok(Coverage::CompleteAsOf(row.synced_up_to))
-        } else {
-            Ok(Coverage::PartialUpTo(row.synced_up_to))
-        }
-    }
-
-    fn list_watermarks_for_relay<'a>(
-        &'a self,
-        relay_url: &str,
-    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
-    {
-        let st = self.lock()?;
-        let rows: Vec<WatermarkRow> = st.watermarks.values()
-            .filter(|r| r.key.relay_url == relay_url)
-            .cloned()
-            .collect();
-        Ok(Box::new(rows.into_iter().map(Ok)))
-    }
-
-    // ─── Hot-set / claims ────────────────────────────────────────────────────
-
-    fn register_view_cover(
-        &self,
-        claimer: ClaimerId,
-        cover_budget: usize,
-    ) -> Result<(), StoreError> {
-        let mut st = self.lock()?;
-        st.claim_budgets.insert(claimer, cover_budget);
-        Ok(())
-    }
-
-    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
-        let mut st = self.lock()?;
-        let ceiling = *st.claim_budgets.get(&claimer).unwrap_or(&DEFAULT_VIEW_CEILING);
-        let current = st.claims.get(&claimer).map(|v| v.len()).unwrap_or(0);
-        let requested = current + ids.len();
-        if requested > ceiling {
-            return Err(StoreError::OverPinned { claimer, requested, ceiling });
-        }
-        let entry = st.claims.entry(claimer).or_default();
-        for id in ids {
-            let hex = bytes_to_hex(id);
-            if !entry.contains(&hex) {
-                entry.push(hex);
-            }
-        }
-        Ok(())
-    }
-
-    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
-        let mut st = self.lock()?;
-        st.claims.remove(&claimer);
-        Ok(())
-    }
-
-    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
-        // Memory backend has no LRU — all events are equally hot. No-op.
-        Ok(())
-    }
-
-    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
-        let start = std::time::Instant::now();
-        let mut st = self.lock()?;
-        let mut report = GcReport::default();
-
-        let now_ms = std::time::SystemTime::now()
-            .duration_since(std::time::UNIX_EPOCH)
-            .map(|d| d.as_millis() as u64)
-            .unwrap_or(0);
-        let now_secs = now_ms / 1000;
-
-        // Reap NIP-40 expired events.
-        let expired_ids: Vec<String> = st.events.iter()
-            .filter(|(_, ev)| ev.raw.expiration().is_some_and(|exp| exp <= now_secs))
-            .map(|(id, _)| id.clone())
-            .take(budget.max_events_per_step)
-            .collect();
-
-        for id_hex in &expired_ids {
-            if let Some(ev) = st.events.remove(id_hex) {
-                st.provenance.remove(id_hex);
-                st.tombstones.insert(id_hex.clone(), TombstoneRow {
-                    target_id: ev.raw.id_bytes(),
-                    kind5_event_id: None,
-                    deleter_pubkey: None,
-                    deleted_at: now_secs,
-                    sources: vec![],
-                    origin: TombstoneOrigin::NIP40Expiry,
-                });
-                report.expired_reaped += 1;
-            }
-            if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
-                break;
-            }
-        }
-
-        // Purge tombstones older than TOMBSTONE_MAX_AGE_SECS.
-        let stale_tombstones: Vec<String> = st.tombstones.iter()
-            .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
-            .map(|(k, _)| k.clone())
-            .collect();
-        report.tombstones_purged = stale_tombstones.len();
-        for k in stale_tombstones {
-            st.tombstones.remove(&k);
-        }
-
-        report.duration_ms = start.elapsed().as_millis() as u32;
-        Ok(report)
-    }
-
-    // ─── Domain rows ─────────────────────────────────────────────────────────
-
-    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
-        let mut st = self.lock()?;
-        let data = st.domain_data
-            .entry(namespace)
-            .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
-            .clone();
-        Ok(DomainHandle {
-            inner: DomainHandleInner::Mem { namespace, data },
-        })
-    }
-
-    fn run_migrations(
-        &self,
-        namespace: &'static str,
-        target_version: u32,
-        migrations: &[DomainMigration],
-    ) -> Result<(), StoreError> {
-        let mut st = self.lock()?;
-        let current = *st.domain_versions.get(namespace).unwrap_or(&0);
-
-        if current > target_version {
-            return Err(StoreError::SchemaTooNew {
-                namespace: namespace.to_string(),
-                on_disk: current,
-                expected: target_version,
-            });
-        }
-
-        if current == target_version {
-            return Ok(());
-        }
-
-        // Get or create domain data arc.
-        let data_arc = st.domain_data
-            .entry(namespace)
-            .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
-            .clone();
-
-        // Apply migrations in order.
-        for m in migrations {
-            if m.from_version < current || m.from_version >= target_version {
-                continue;
-            }
-            let mut tx = crate::substrate::MigrationTx::default();
-            (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed {
-                namespace: namespace.to_string(),
-                from: m.from_version,
-                to: m.to_version,
-                reason,
-            })?;
-            let mut data = data_arc.lock().map_err(|e| StoreError::Io(e.to_string()))?;
-            for (k, v) in tx.writes() {
-                data.insert(k.clone(), v.clone());
-            }
-        }
-
-        st.domain_versions.insert(namespace, target_version);
-        Ok(())
-    }
-
-    // ─── Export ──────────────────────────────────────────────────────────────
-
-    fn dump(
-        &self,
-        out: &mut dyn std::io::Write,
-        format: DumpFormat,
-    ) -> Result<DumpStats, StoreError> {
-        if !matches!(format, DumpFormat::Jsonl) {
-            return Err(StoreError::Io("CBOR dump not yet implemented".into()));
-        }
-
-        let st = self.lock()?;
-        let mut stats = DumpStats::default();
-
-        // Dump events in deterministic order (ascending hex id).
-        let mut event_ids: Vec<&String> = st.events.keys().collect();
-        event_ids.sort();
-        for id in event_ids {
-            let ev = &st.events[id];
-            let line = serde_json::json!({
-                "type": "event",
-                "event": *ev.raw,
-                "received_at_ms": ev.received_at_ms,
-            })
-            .to_string();
-            let bytes = (line + "\n").into_bytes();
-            stats.bytes_written += bytes.len() as u64;
-            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
-            stats.events += 1;
-        }
-
-        // Dump tombstones in deterministic order.
-        let mut tomb_ids: Vec<&String> = st.tombstones.keys().collect();
-        tomb_ids.sort();
-        for id in tomb_ids {
-            let t = &st.tombstones[id];
-            let line = serde_json::json!({
-                "type": "tombstone",
-                "target_id": bytes_to_hex(&t.target_id),
-                "deleted_at": t.deleted_at,
-                "origin": format!("{:?}", t.origin),
-            })
-            .to_string();
-            let bytes = (line + "\n").into_bytes();
-            stats.bytes_written += bytes.len() as u64;
-            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
-            stats.tombstones += 1;
-        }
-
-        // Dump watermarks in deterministic order.
-        let mut wm_keys: Vec<&(String, String)> = st.watermarks.keys().collect();
-        wm_keys.sort();
-        for k in wm_keys {
-            let r = &st.watermarks[k];
-            let line = serde_json::json!({
-                "type": "watermark",
-                "filter_hash": &r.key.filter_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
-                "relay_url": &r.key.relay_url,
-                "synced_up_to": r.synced_up_to,
-            })
-            .to_string();
-            let bytes = (line + "\n").into_bytes();
-            stats.bytes_written += bytes.len() as u64;
-            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
-            stats.watermarks += 1;
-        }
-
-        // Dump domain rows in deterministic order (namespace, key).
-        let mut ns_list: Vec<&&'static str> = st.domain_data.keys().collect();
-        ns_list.sort();
-        for ns in ns_list {
-            let data = st.domain_data[ns].lock().map_err(|e| StoreError::Io(e.to_string()))?;
-            let mut pairs: Vec<(&Vec<u8>, &Vec<u8>)> = data.iter().collect();
-            pairs.sort_by_key(|(k, _)| *k);
-            for (k, v) in pairs {
-                let line = serde_json::json!({
-                    "type": "domain",
-                    "namespace": ns,
-                    "key": k,
-                    "value": v,
-                })
-                .to_string();
-                let bytes = (line + "\n").into_bytes();
-                stats.bytes_written += bytes.len() as u64;
-                out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
-                stats.domain_rows += 1;
-            }
-        }
-
-        Ok(stats)
-    }
-}
-
-// ─── Insert helpers ───────────────────────────────────────────────────────────
-
-fn handle_normal_insert(
-    st: &mut MemState,
-    event: RawEvent,
-    source: &RelayUrl,
-    received_at_ms: u64,
-) -> Result<InsertOutcome, StoreError> {
-    let id_bytes = event.id_bytes();
-    let id_hex = event.id.clone();
-
-    if let Some(_existing) = st.events.get(&id_hex) {
-        // Duplicate: merge provenance only.
-        let p = st.provenance.entry(id_hex.clone()).or_default();
-        upsert_provenance(p, source.clone(), received_at_ms);
-        let sources_after = p.len() as u32;
-        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after });
-    }
-
-    let stored = StoredEvent {
-        raw: Arc::new(event),
-        received_at_ms,
-    };
-    st.events.insert(id_hex.clone(), stored);
-
-    let p = st.provenance.entry(id_hex).or_default();
-    upsert_provenance(p, source.clone(), received_at_ms);
-    let sources_after = p.len() as u32;
-
-    Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
-}
-
-fn handle_replaceable_insert(
-    st: &mut MemState,
-    event: RawEvent,
-    source: &RelayUrl,
-    received_at_ms: u64,
-) -> Result<InsertOutcome, StoreError> {
-    let id_bytes = event.id_bytes();
-    let id_hex = event.id.clone();
-    let pubkey_hex = event.pubkey.clone();
-    let kind = event.kind;
-
-    // Check for exact-id duplicate first — this supersedes the replaceable logic.
-    if st.events.contains_key(&id_hex) {
-        let p = st.provenance.entry(id_hex).or_default();
-        upsert_provenance(p, source.clone(), received_at_ms);
-        let sources_after = p.len() as u32;
-        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after });
-    }
-
-    // Find existing replaceable for this (pubkey, kind).
-    let existing_id: Option<String> = st.events.iter()
-        .filter(|(_, ev)| ev.raw.pubkey == pubkey_hex && ev.raw.kind == kind)
-        .max_by(|(_, a), (_, b)| {
-            a.raw.created_at.cmp(&b.raw.created_at)
-                .then(b.raw.id.cmp(&a.raw.id))
-        })
-        .map(|(id, _)| id.clone());
-
-    if let Some(ref existing_hex) = existing_id {
-        let existing_ev = &st.events[existing_hex];
-        let existing_time = existing_ev.raw.created_at;
-        let existing_id_str = existing_ev.raw.id.clone();
-
-        // Determine winner: newer created_at wins; tie → smaller id wins.
-        let incoming_wins = event.created_at > existing_time
-            || (event.created_at == existing_time && event.id < existing_id_str);
-
-        if incoming_wins {
-            // Remove old event.
-            let replaced_id = hex_to_bytes32_owned(existing_hex);
-            st.events.remove(existing_hex);
-            st.provenance.remove(existing_hex);
-
-            // Insert new.
-            let new_id = id_bytes;
-            let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
-            st.events.insert(id_hex.clone(), stored);
-            let p = st.provenance.entry(id_hex).or_default();
-            upsert_provenance(p, source.clone(), received_at_ms);
-
-            Ok(InsertOutcome::Replaced { new_id, replaced_id })
-        } else {
-            // Incoming is older — drop it.
-            let current_id = hex_to_bytes32_owned(existing_hex);
-            Ok(InsertOutcome::Superseded { id: id_bytes, current_id })
-        }
-    } else {
-        // No existing — fresh insert.
-        let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
-        st.events.insert(id_hex.clone(), stored);
-        let p = st.provenance.entry(id_hex).or_default();
-        upsert_provenance(p, source.clone(), received_at_ms);
-        let sources_after = p.len() as u32;
-        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
-    }
-}
-
-fn handle_param_replaceable_insert(
-    st: &mut MemState,
-    event: RawEvent,
-    source: &RelayUrl,
-    received_at_ms: u64,
-) -> Result<InsertOutcome, StoreError> {
-    let id_bytes = event.id_bytes();
-    let id_hex = event.id.clone();
-    let pubkey_hex = event.pubkey.clone();
-    let kind = event.kind;
-    let d_tag = event.d_tag().unwrap_or_default();
-    let d_str = String::from_utf8_lossy(&d_tag).into_owned();
-
-    // Check for exact-id duplicate first — this supersedes the replaceable logic.
-    if st.events.contains_key(&id_hex) {
-        let p = st.provenance.entry(id_hex).or_default();
-        upsert_provenance(p, source.clone(), received_at_ms);
-        let sources_after = p.len() as u32;
-        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after });
-    }
-
-    // Find existing parameterized replaceable for (pubkey, kind, d_tag).
-    let existing_id: Option<String> = st.events.iter()
-        .filter(|(_, ev)| {
-            ev.raw.pubkey == pubkey_hex
-                && ev.raw.kind == kind
-                && ev.raw.d_tag()
-                    .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
-                    .unwrap_or(false)
-        })
-        .max_by(|(_, a), (_, b)| {
-            a.raw.created_at.cmp(&b.raw.created_at)
-                .then(b.raw.id.cmp(&a.raw.id))
-        })
-        .map(|(id, _)| id.clone());
-
-    if let Some(ref existing_hex) = existing_id {
-        let existing_ev = &st.events[existing_hex];
-        let existing_time = existing_ev.raw.created_at;
-        let existing_id_str = existing_ev.raw.id.clone();
-
-        let incoming_wins = event.created_at > existing_time
-            || (event.created_at == existing_time && event.id < existing_id_str);
-
-        if incoming_wins {
-            let replaced_id = hex_to_bytes32_owned(existing_hex);
-            st.events.remove(existing_hex);
-            st.provenance.remove(existing_hex);
-
-            let new_id = id_bytes;
-            let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
-            st.events.insert(id_hex.clone(), stored);
-            let p = st.provenance.entry(id_hex).or_default();
-            upsert_provenance(p, source.clone(), received_at_ms);
-
-            Ok(InsertOutcome::Replaced { new_id, replaced_id })
-        } else {
-            let current_id = hex_to_bytes32_owned(existing_hex);
-            Ok(InsertOutcome::Superseded { id: id_bytes, current_id })
-        }
-    } else {
-        let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
-        st.events.insert(id_hex.clone(), stored);
-        let p = st.provenance.entry(id_hex).or_default();
-        upsert_provenance(p, source.clone(), received_at_ms);
-        let sources_after = p.len() as u32;
-        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
-    }
-}
-
-fn handle_kind5_insert(
-    st: &mut MemState,
-    event: RawEvent,
-    source: &RelayUrl,
-    received_at_ms: u64,
-) -> Result<InsertOutcome, StoreError> {
-    let kind5_id_bytes = event.id_bytes();
-    let kind5_id_hex = event.id.clone();
-    let kind5_pubkey = event.pubkey.clone();
-    let kind5_created_at = event.created_at;
-
-    // Process `e`-tag deletes.
-    for target_hex in event.e_tags() {
-        if let Some(existing) = st.events.get(&target_hex) {
-            // Only self-deletes: deleter must own the target.
-            if existing.raw.pubkey != kind5_pubkey {
-                continue;
-            }
-            let target_id = existing.raw.id_bytes();
-            st.events.remove(&target_hex);
-            st.provenance.remove(&target_hex);
-            st.tombstones.insert(target_hex, TombstoneRow {
-                target_id,
-                kind5_event_id: Some(kind5_id_bytes),
-                deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
-                deleted_at: kind5_created_at,
-                sources: vec![source.clone()],
-                origin: TombstoneOrigin::Kind5,
-            });
-        } else {
-            // Target doesn't exist yet — write a pre-emptive tombstone.
-            let target_bytes = hex_to_bytes32_owned(&target_hex);
-            st.tombstones.entry(target_hex).or_insert(TombstoneRow {
-                target_id: target_bytes,
-                kind5_event_id: Some(kind5_id_bytes),
-                deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
-                deleted_at: kind5_created_at,
-                sources: vec![source.clone()],
-                origin: TombstoneOrigin::Kind5,
-            });
-        }
-    }
-
-    // Process `a`-tag deletes (parameterized replaceables).
-    for addr in event.a_tags() {
-        // addr format: "kind:pubkey:dtag"
-        let parts: Vec<&str> = addr.splitn(3, ':').collect();
-        if parts.len() < 3 { continue; }
-        let target_kind_str = parts[0];
-        let target_pubkey = parts[1];
-        let target_dtag = parts[2];
-
-        // Only self-deletes.
-        if target_pubkey != kind5_pubkey { continue; }
-
-        let addr_key = format!("{}:{}:{}", target_kind_str, target_pubkey, target_dtag);
-        let Ok(target_kind) = target_kind_str.parse::<u32>() else { continue };
-
-        // Delete any existing matching parameterized replaceable.
-        let to_delete: Vec<String> = st.events.iter()
-            .filter(|(_, ev)| {
-                ev.raw.pubkey == target_pubkey
-                    && ev.raw.kind == target_kind
-                    && ev.raw.d_tag()
-                        .map(|d| String::from_utf8_lossy(&d).into_owned() == target_dtag)
-                        .unwrap_or(false)
-                    && ev.raw.created_at <= kind5_created_at
-            })
-            .map(|(id, _)| id.clone())
-            .collect();
-
-        for target_hex in to_delete {
-            if let Some(existing) = st.events.remove(&target_hex) {
-                st.provenance.remove(&target_hex);
-                st.tombstones.insert(target_hex, TombstoneRow {
-                    target_id: existing.raw.id_bytes(),
-                    kind5_event_id: Some(kind5_id_bytes),
-                    deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
-                    deleted_at: kind5_created_at,
-                    sources: vec![source.clone()],
-                    origin: TombstoneOrigin::Kind5,
-                });
-            }
-        }
-
-        // Write address tombstone for events arriving later.
-        st.addr_tombstones.entry(addr_key).or_insert(TombstoneRow {
-            target_id: [0u8; 32], // address tombstone has no specific target id
-            kind5_event_id: Some(kind5_id_bytes),
-            deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
-            deleted_at: kind5_created_at,
-            sources: vec![source.clone()],
-            origin: TombstoneOrigin::Kind5,
-        });
-    }
-
-    // Store the kind:5 event itself.
-    let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
-    st.events.insert(kind5_id_hex.clone(), stored);
-    let p = st.provenance.entry(kind5_id_hex).or_default();
-    upsert_provenance(p, source.clone(), received_at_ms);
-    let sources_after = p.len() as u32;
-
-    Ok(InsertOutcome::Inserted { id: kind5_id_bytes, sources_after })
-}
-
-// ─── Utilities ────────────────────────────────────────────────────────────────
-
-fn bytes_to_hex(b: &[u8]) -> String {
-    b.iter().map(|byte| format!("{byte:02x}")).collect()
-}
-
-fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
-    let mut out = [0u8; 32];
-    if s.len() != 64 { return out; }
-    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
-        if i >= 32 { break; }
-        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
-            out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
-        }
-    }
-    out
-}
-
-fn hex_nibble(b: u8) -> u8 {
-    match b {
-        b'0'..=b'9' => b - b'0',
-        b'a'..=b'f' => b - b'a' + 10,
-        b'A'..=b'F' => b - b'A' + 10,
-        _ => 0,
-    }
-}
diff --git a/crates/nmp-core/src/store/mem/domain.rs b/crates/nmp-core/src/store/mem/domain.rs
new file mode 100644
index 0000000..7b41b2d
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/domain.rs
@@ -0,0 +1,79 @@
+//! Domain rows and migration support for `MemEventStore`.
+//!
+//! D0: domain isolation — each module gets its own namespace handle.
+//! One `DomainHandle` cannot read another module's namespace.
+
+use std::collections::HashMap;
+use std::sync::{Arc, Mutex};
+
+use super::MemEventStore;
+use crate::store::events::{DomainHandle, DomainHandleInner};
+use crate::store::StoreError;
+use crate::substrate::DomainMigration;
+
+pub(super) fn domain_open(
+    store: &MemEventStore,
+    namespace: &'static str,
+) -> Result<DomainHandle, StoreError> {
+    let mut st = store.lock()?;
+    let data = st
+        .domain_data
+        .entry(namespace)
+        .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
+        .clone();
+    Ok(DomainHandle {
+        inner: DomainHandleInner::Mem { namespace, data },
+    })
+}
+
+pub(super) fn run_migrations(
+    store: &MemEventStore,
+    namespace: &'static str,
+    target_version: u32,
+    migrations: &[DomainMigration],
+) -> Result<(), StoreError> {
+    let mut st = store.lock()?;
+    let current = *st.domain_versions.get(namespace).unwrap_or(&0);
+
+    if current > target_version {
+        return Err(StoreError::SchemaTooNew {
+            namespace: namespace.to_string(),
+            on_disk: current,
+            expected: target_version,
+        });
+    }
+
+    if current == target_version {
+        return Ok(());
+    }
+
+    // Get or create domain data arc.
+    let data_arc = st
+        .domain_data
+        .entry(namespace)
+        .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
+        .clone();
+
+    // Apply migrations in order.
+    for m in migrations {
+        if m.from_version < current || m.from_version >= target_version {
+            continue;
+        }
+        let mut tx = crate::substrate::MigrationTx::default();
+        (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed {
+            namespace: namespace.to_string(),
+            from: m.from_version,
+            to: m.to_version,
+            reason,
+        })?;
+        let mut data = data_arc
+            .lock()
+            .map_err(|e| StoreError::Io(e.to_string()))?;
+        for (k, v) in tx.writes() {
+            data.insert(k.clone(), v.clone());
+        }
+    }
+
+    st.domain_versions.insert(namespace, target_version);
+    Ok(())
+}
diff --git a/crates/nmp-core/src/store/mem/gc.rs b/crates/nmp-core/src/store/mem/gc.rs
new file mode 100644
index 0000000..fff7051
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/gc.rs
@@ -0,0 +1,194 @@
+//! Claim / release / gc_step for `MemEventStore`.
+//!
+//! Implements the HotSet semantics from `docs/design/lmdb/gc.md` §2:
+//!   - per-view ceiling: `DEFAULT_VIEW_CEILING` (1000 events).
+//!   - global pinned ceiling: `MAX_PINNED_TOTAL` (20000 events).
+//!   - BTreeSet idempotency per T25: re-claiming a known id is a no-op.
+//!   - `StoreError::OverPinned` on breach (D8).
+
+use super::{bytes_to_hex, MemEventStore, DEFAULT_VIEW_CEILING, MAX_PINNED_TOTAL, TOMBSTONE_MAX_AGE_SECS};
+use crate::store::types::{
+    ClaimerId, EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow,
+};
+use crate::store::StoreError;
+
+pub(super) fn register_view_cover(
+    store: &MemEventStore,
+    claimer: ClaimerId,
+    cover_budget: usize,
+) -> Result<(), StoreError> {
+    let mut st = store.lock()?;
+    st.claim_budgets.insert(claimer, cover_budget);
+    Ok(())
+}
+
+pub(super) fn claim(
+    store: &MemEventStore,
+    claimer: ClaimerId,
+    ids: &[EventId],
+) -> Result<(), StoreError> {
+    let mut st = store.lock()?;
+    let ceiling = *st.claim_budgets.get(&claimer).unwrap_or(&DEFAULT_VIEW_CEILING);
+
+    let existing_set = st.claims.entry(claimer).or_default();
+    // Only count genuinely new ids (BTreeSet idempotency).
+    let new_ids: Vec<String> = ids
+        .iter()
+        .map(|id| bytes_to_hex(id))
+        .filter(|hex| !existing_set.contains(hex))
+        .collect();
+
+    let current_for_claimer = existing_set.len();
+    let requested_for_claimer = current_for_claimer + new_ids.len();
+    if requested_for_claimer > ceiling {
+        return Err(StoreError::OverPinned {
+            claimer,
+            requested: requested_for_claimer,
+            ceiling,
+        });
+    }
+
+    // Global pinned ceiling check (D8 / gc.md §2).
+    let all_pinned: usize = st.claims.values().map(|s| s.len()).sum();
+    let global_new = new_ids
+        .iter()
+        .filter(|hex| !st.claims.values().any(|s| s.contains(*hex)))
+        .count();
+    let requested_global = all_pinned + global_new;
+    if requested_global > MAX_PINNED_TOTAL {
+        return Err(StoreError::OverPinned {
+            claimer,
+            requested: requested_global,
+            ceiling: MAX_PINNED_TOTAL,
+        });
+    }
+
+    // Apply the claims.
+    let set = st.claims.entry(claimer).or_default();
+    for hex in new_ids {
+        set.insert(hex);
+    }
+    Ok(())
+}
+
+pub(super) fn release(
+    store: &MemEventStore,
+    claimer: ClaimerId,
+) -> Result<(), StoreError> {
+    let mut st = store.lock()?;
+    st.claims.remove(&claimer);
+    // Leave budget registered — re-registering at re-open is the actor's job.
+    Ok(())
+}
+
+pub(super) fn gc_step(
+    store: &MemEventStore,
+    budget: GcBudget,
+) -> Result<GcReport, StoreError> {
+    let start = std::time::Instant::now();
+    let mut st = store.lock()?;
+    let mut report = GcReport::default();
+
+    let now_ms = std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map(|d| d.as_millis() as u64)
+        .unwrap_or(0);
+    let now_secs = now_ms / 1000;
+
+    // Reap NIP-40 expired events.
+    let expired_ids: Vec<String> = st
+        .events
+        .iter()
+        .filter(|(_, ev)| ev.raw.expiration().is_some_and(|exp| exp <= now_secs))
+        .map(|(id, _)| id.clone())
+        .take(budget.max_events_per_step)
+        .collect();
+
+    for id_hex in &expired_ids {
+        if let Some(ev) = st.events.remove(id_hex) {
+            st.provenance.remove(id_hex);
+            st.tombstones.insert(
+                id_hex.clone(),
+                TombstoneRow {
+                    target_id: ev.raw.id_bytes(),
+                    kind5_event_id: None,
+                    deleter_pubkey: None,
+                    deleted_at: now_secs,
+                    sources: vec![],
+                    origin: TombstoneOrigin::NIP40Expiry,
+                },
+            );
+            report.expired_reaped += 1;
+        }
+        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
+            break;
+        }
+    }
+
+    // Purge tombstones older than TOMBSTONE_MAX_AGE_SECS.
+    let stale_tombstones: Vec<String> = st
+        .tombstones
+        .iter()
+        .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
+        .map(|(k, _)| k.clone())
+        .collect();
+    report.tombstones_purged = stale_tombstones.len();
+    for k in stale_tombstones {
+        st.tombstones.remove(&k);
+    }
+
+    report.duration_ms = start.elapsed().as_millis() as u32;
+    Ok(report)
+}
+
+// ─── Tests ───────────────────────────────────────────────────────────────────
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+    use crate::store::{EventStore, MemEventStore};
+    use crate::store::types::EventId;
+
+    fn make_id(b: u8) -> EventId {
+        let mut id = [0u8; 32];
+        id[0] = b;
+        id
+    }
+
+    #[test]
+    fn claim_idempotent_reclaim_does_not_count() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(1);
+        store.register_view_cover(c, 5).unwrap();
+        let id = make_id(1);
+        store.claim(c, &[id]).unwrap();
+        // Re-claiming the same id must not count toward the ceiling.
+        store.claim(c, &[id]).unwrap();
+        let st = store.lock().unwrap();
+        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
+    }
+
+    #[test]
+    fn claim_over_per_view_ceiling_returns_err() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(2);
+        store.register_view_cover(c, 2).unwrap();
+        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
+        let result = store.claim(c, &[make_id(3)]);
+        assert!(
+            matches!(result, Err(StoreError::OverPinned { .. })),
+            "must return OverPinned when per-view ceiling exceeded"
+        );
+    }
+
+    #[test]
+    fn release_clears_all_pins() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(3);
+        store.register_view_cover(c, 100).unwrap();
+        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
+        store.release(c).unwrap();
+        let st = store.lock().unwrap();
+        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
+    }
+}
diff --git a/crates/nmp-core/src/store/mem/insert.rs b/crates/nmp-core/src/store/mem/insert.rs
new file mode 100644
index 0000000..74e48c4
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/insert.rs
@@ -0,0 +1,357 @@
+//! §7.1 insert invariants for `MemEventStore`.
+//!
+//! D4: ONE writer. All event mutations flow through here.
+//! D2: Returns typed `InsertOutcome`; never panics.
+//!
+//! P2 fixes applied here:
+//!   - Duplicate check BEFORE kind-specific supersession (provenance merge).
+//!   - Tombstone max-merge (`deleted_at` max + source union instead of or_insert).
+
+use std::collections::HashMap;
+use std::sync::Arc;
+
+use super::{bytes_to_hex, upsert_provenance, MemEventStore, MemState};
+use crate::store::types::{
+    DeleteFilter, InsertOutcome, RawEvent, RejectReason, RelayUrl, StoredEvent,
+    TombstoneOrigin, TombstoneRow,
+};
+use crate::store::StoreError;
+
+// ─── Public entry points ─────────────────────────────────────────────────────
+
+pub(super) fn insert(
+    store: &MemEventStore,
+    event: RawEvent,
+    source: &RelayUrl,
+    received_at_ms: u64,
+) -> Result<InsertOutcome, StoreError> {
+    // 1. Structural validation (sig check deferred to nostr crate wiring).
+    if !event.is_structurally_valid() {
+        return Ok(InsertOutcome::Rejected {
+            id: event.id_bytes(),
+            reason: RejectReason::Malformed("invalid id/pubkey/sig length".into()),
+        });
+    }
+
+    // 2. Ephemeral: deliver to live consumers, do not store.
+    if event.is_ephemeral() {
+        return Ok(InsertOutcome::Ephemeral { id: event.id_bytes() });
+    }
+
+    // 3. Check NIP-40 expiration on arrival.
+    if let Some(exp) = event.expiration() {
+        let now_secs = received_at_ms / 1000;
+        if exp <= now_secs {
+            return Ok(InsertOutcome::Rejected {
+                id: event.id_bytes(),
+                reason: RejectReason::ExpiredOnArrival,
+            });
+        }
+    }
+
+    let id_bytes = event.id_bytes();
+    let id_hex = event.id.clone();
+    let mut st = store.lock()?;
+
+    // 4. Check per-id tombstone.
+    // Foreign kind:5 pre-tombstones (deleter != author) must NOT block the event.
+    if let Some(tomb) = st.tombstones.get(&id_hex).cloned() {
+        let applies = match tomb.origin {
+            TombstoneOrigin::Kind5 => tomb
+                .deleter_pubkey
+                .as_ref()
+                .map(|dp| bytes_to_hex(dp) == event.pubkey)
+                .unwrap_or(false),
+            TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
+        };
+        if applies {
+            return Ok(InsertOutcome::Tombstoned {
+                id: id_bytes,
+                kind5_event_id: tomb.kind5_event_id,
+                origin: tomb.origin,
+            });
+        }
+        // Foreign pre-tombstone — remove and allow insert (invariant 3).
+        st.tombstones.remove(&id_hex);
+    }
+
+    // 5. Check address tombstone for parameterized replaceables.
+    if event.is_param_replaceable() {
+        if let Some(d) = event.d_tag() {
+            let addr_key = format!(
+                "{}:{}:{}",
+                event.kind,
+                event.pubkey,
+                String::from_utf8_lossy(&d)
+            );
+            if let Some(tomb) = st.addr_tombstones.get(&addr_key) {
+                if tomb.deleted_at >= event.created_at {
+                    return Ok(InsertOutcome::Tombstoned {
+                        id: id_bytes,
+                        kind5_event_id: tomb.kind5_event_id,
+                        origin: tomb.origin,
+                    });
+                }
+            }
+        }
+    }
+
+    // 6. Kind:5 self-delete handling.
+    if event.kind == 5 {
+        return handle_kind5_insert(&mut st, event, source, received_at_ms);
+    }
+
+    // 7. Replaceable supersession.
+    if event.is_replaceable() {
+        let key = (event.pubkey.clone(), event.kind, None::<String>);
+        return handle_supersession(&mut st, event, source, received_at_ms, key);
+    }
+
+    // 8. Parameterized replaceable.
+    if event.is_param_replaceable() {
+        let d = event.d_tag()
+            .map(|b| String::from_utf8_lossy(&b).into_owned());
+        let key = (event.pubkey.clone(), event.kind, d);
+        return handle_supersession(&mut st, event, source, received_at_ms, key);
+    }
+
+    // 9. Normal insert / duplicate.
+    handle_normal_insert(&mut st, event, source, received_at_ms)
+}
+
+pub(super) fn delete_by_filter(
+    store: &MemEventStore,
+    filter: DeleteFilter,
+) -> Result<usize, StoreError> {
+    let mut st = store.lock()?;
+    let ids_to_remove: Vec<String> = match &filter {
+        DeleteFilter::ByRelayOnly(relay) => st
+            .events
+            .keys()
+            .filter(|id| {
+                st.provenance
+                    .get(*id)
+                    .map(|p| p.len() == 1 && p[0].relay_url == *relay)
+                    .unwrap_or(false)
+            })
+            .cloned()
+            .collect(),
+        DeleteFilter::ByAuthor(pk) => {
+            let pk_hex = bytes_to_hex(pk);
+            st.events
+                .iter()
+                .filter(|(_, ev)| ev.raw.pubkey == pk_hex)
+                .map(|(id, _)| id.clone())
+                .collect()
+        }
+        DeleteFilter::ByIds(ids) => ids
+            .iter()
+            .map(|id| bytes_to_hex(id))
+            .filter(|h| st.events.contains_key(h))
+            .collect(),
+        DeleteFilter::ByKindRange { lo, hi } => st
+            .events
+            .iter()
+            .filter(|(_, ev)| ev.raw.kind >= *lo && ev.raw.kind <= *hi)
+            .map(|(id, _)| id.clone())
+            .collect(),
+    };
+    let count = ids_to_remove.len();
+    for id in ids_to_remove {
+        st.events.remove(&id);
+        st.provenance.remove(&id);
+    }
+    Ok(count)
+}
+
+// ─── Shared supersession helper ───────────────────────────────────────────────
+
+/// Unified supersession logic for both replaceable and param-replaceable kinds.
+/// `key` = (pubkey_hex, kind, Option<d_tag_str>) — None means any d-tag (replaceable).
+fn handle_supersession(
+    st: &mut MemState,
+    event: RawEvent,
+    source: &RelayUrl,
+    received_at_ms: u64,
+    key: (String, u32, Option<String>),
+) -> Result<InsertOutcome, StoreError> {
+    let id_bytes = event.id_bytes();
+    let id_hex = event.id.clone();
+    let (pubkey_hex, kind, d_tag_filter) = key;
+
+    // P2 fix: exact-id duplicate BEFORE supersession check.
+    if st.events.contains_key(&id_hex) {
+        let p = st.provenance.entry(id_hex).or_default();
+        upsert_provenance(p, source.clone(), received_at_ms);
+        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after: p.len() as u32 });
+    }
+
+    let existing_id: Option<String> = st
+        .events
+        .iter()
+        .filter(|(_, ev)| {
+            ev.raw.pubkey == pubkey_hex
+                && ev.raw.kind == kind
+                && match &d_tag_filter {
+                    None => true,
+                    Some(d) => ev.raw.d_tag()
+                        .map(|tag| String::from_utf8_lossy(&tag).into_owned() == *d)
+                        .unwrap_or(false),
+                }
+        })
+        .max_by(|(_, a), (_, b)| {
+            a.raw.created_at
+                .cmp(&b.raw.created_at)
+                .then(b.raw.id.cmp(&a.raw.id))
+        })
+        .map(|(id, _)| id.clone());
+
+    if let Some(ref existing_hex) = existing_id {
+        let existing_ev = &st.events[existing_hex];
+        let existing_time = existing_ev.raw.created_at;
+        let existing_id_str = existing_ev.raw.id.clone();
+        let incoming_wins = event.created_at > existing_time
+            || (event.created_at == existing_time && event.id < existing_id_str);
+
+        if incoming_wins {
+            let replaced_id = hex_to_bytes32_owned(existing_hex);
+            st.events.remove(existing_hex);
+            st.provenance.remove(existing_hex);
+            let new_id = id_bytes;
+            st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
+            let p = st.provenance.entry(id_hex).or_default();
+            upsert_provenance(p, source.clone(), received_at_ms);
+            Ok(InsertOutcome::Replaced { new_id, replaced_id })
+        } else {
+            Ok(InsertOutcome::Superseded { id: id_bytes, current_id: hex_to_bytes32_owned(existing_hex) })
+        }
+    } else {
+        st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
+        let p = st.provenance.entry(id_hex).or_default();
+        upsert_provenance(p, source.clone(), received_at_ms);
+        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after: p.len() as u32 })
+    }
+}
+
+fn handle_normal_insert(
+    st: &mut MemState,
+    event: RawEvent,
+    source: &RelayUrl,
+    received_at_ms: u64,
+) -> Result<InsertOutcome, StoreError> {
+    let id_bytes = event.id_bytes();
+    let id_hex = event.id.clone();
+
+    if st.events.contains_key(&id_hex) {
+        let p = st.provenance.entry(id_hex.clone()).or_default();
+        upsert_provenance(p, source.clone(), received_at_ms);
+        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after: p.len() as u32 });
+    }
+
+    st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
+    let p = st.provenance.entry(id_hex).or_default();
+    upsert_provenance(p, source.clone(), received_at_ms);
+    Ok(InsertOutcome::Inserted { id: id_bytes, sources_after: p.len() as u32 })
+}
+
+fn handle_kind5_insert(
+    st: &mut MemState,
+    event: RawEvent,
+    source: &RelayUrl,
+    received_at_ms: u64,
+) -> Result<InsertOutcome, StoreError> {
+    let kind5_id_bytes = event.id_bytes();
+    let kind5_id_hex = event.id.clone();
+    let kind5_pubkey = event.pubkey.clone();
+    let kind5_at = event.created_at;
+
+    // Process `e`-tag deletes (self-deletes only).
+    for target_hex in event.e_tags() {
+        if let Some(existing) = st.events.get(&target_hex) {
+            if existing.raw.pubkey != kind5_pubkey { continue; }
+            let target_id = existing.raw.id_bytes();
+            st.events.remove(&target_hex);
+            st.provenance.remove(&target_hex);
+            merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source));
+        } else {
+            let target_id = hex_to_bytes32_owned(&target_hex);
+            merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source));
+        }
+    }
+
+    // Process `a`-tag deletes (parameterized replaceables, self-delete only).
+    for addr in event.a_tags() {
+        let parts: Vec<&str> = addr.splitn(3, ':').collect();
+        if parts.len() < 3 { continue; }
+        let (tgt_kind_str, tgt_pk, tgt_dtag) = (parts[0], parts[1], parts[2]);
+        if tgt_pk != kind5_pubkey { continue; }
+        let Ok(tgt_kind) = tgt_kind_str.parse::<u32>() else { continue };
+        let addr_key = format!("{}:{}:{}", tgt_kind_str, tgt_pk, tgt_dtag);
+
+        let to_delete: Vec<String> = st.events.iter()
+            .filter(|(_, ev)| {
+                ev.raw.pubkey == tgt_pk && ev.raw.kind == tgt_kind
+                    && ev.raw.d_tag().map(|d| String::from_utf8_lossy(&d).into_owned() == tgt_dtag).unwrap_or(false)
+                    && ev.raw.created_at <= kind5_at
+            })
+            .map(|(id, _)| id.clone())
+            .collect();
+
+        for target_hex in to_delete {
+            if let Some(existing) = st.events.remove(&target_hex) {
+                st.provenance.remove(&target_hex);
+                merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(existing.raw.id_bytes(), kind5_id_bytes, &kind5_pubkey, kind5_at, source));
+            }
+        }
+        // Address tombstone for events arriving later (max-merge).
+        merge_tombstone(&mut st.addr_tombstones, addr_key, kind5_tomb([0u8; 32], kind5_id_bytes, &kind5_pubkey, kind5_at, source));
+    }
+
+    // Store the kind:5 event itself.
+    st.events.insert(kind5_id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
+    let p = st.provenance.entry(kind5_id_hex).or_default();
+    upsert_provenance(p, source.clone(), received_at_ms);
+    Ok(InsertOutcome::Inserted { id: kind5_id_bytes, sources_after: p.len() as u32 })
+}
+
+// ─── Tombstone helpers ────────────────────────────────────────────────────────
+
+fn kind5_tomb(
+    target_id: [u8; 32],
+    kind5_id: [u8; 32],
+    kind5_pubkey: &str,
+    deleted_at: u64,
+    source: &RelayUrl,
+) -> TombstoneRow {
+    TombstoneRow {
+        target_id,
+        kind5_event_id: Some(kind5_id),
+        deleter_pubkey: Some(hex_to_bytes32_owned(kind5_pubkey)),
+        deleted_at,
+        sources: vec![source.clone()],
+        origin: TombstoneOrigin::Kind5,
+    }
+}
+
+/// P2 fix: tombstone upsert max-merges `deleted_at` and unions sources.
+/// Original `or_insert` kept first-arrived timestamp — wrong for re-deliveries.
+fn merge_tombstone(map: &mut HashMap<String, TombstoneRow>, key: String, incoming: TombstoneRow) {
+    match map.get_mut(&key) {
+        Some(existing) => {
+            if incoming.deleted_at > existing.deleted_at {
+                existing.deleted_at = incoming.deleted_at;
+                existing.kind5_event_id = incoming.kind5_event_id;
+            }
+            for src in incoming.sources {
+                if !existing.sources.contains(&src) {
+                    existing.sources.push(src);
+                }
+            }
+        }
+        None => { map.insert(key, incoming); }
+    }
+}
+
+fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
+    RawEvent::hex_to_bytes32_owned(s)
+}
diff --git a/crates/nmp-core/src/store/mem/mod.rs b/crates/nmp-core/src/store/mem/mod.rs
new file mode 100644
index 0000000..7b474b3
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/mod.rs
@@ -0,0 +1,193 @@
+//! In-memory `EventStore` backend.
+//!
+//! Used for tests and the pre-M15 web target. Every method is fully implemented
+//! against a `Mutex<MemState>` so tests cover the same logic that the LMDB
+//! backend will eventually call.
+//!
+//! See `docs/design/lmdb/trait.md` §5 ("Two backends in v1").
+//!
+//! Module layout (Article I — each sub-module ≤ 300 LOC):
+//!   mod.rs      — factory, `MemState`, `MemEventStore`, provenance helpers
+//!   store_impl.rs — `EventStore` trait impl (delegation to sub-modules)
+//!   insert.rs   — §7.1 insert invariants (replaceable, kind:5, normal)
+//!   query.rs    — read / scan methods
+//!   gc.rs       — claim / release / prune
+//!   domain.rs   — domain rows + migrations
+
+pub(super) mod domain;
+pub(super) mod gc;
+pub(super) mod insert;
+pub(super) mod query;
+mod store_impl;
+#[cfg(test)]
+mod tests;
+
+use std::collections::{BTreeSet, HashMap};
+use std::sync::{Arc, Mutex};
+
+use super::types::{
+    ClaimerId, ProvenanceEntry, RelayUrl, StoredEvent, TombstoneRow, WatermarkRow,
+};
+use super::StoreError;
+
+// ─── Constants ───────────────────────────────────────────────────────────────
+
+/// Default maximum pinned events per view (D8 / gc.md §2).
+pub(super) const DEFAULT_VIEW_CEILING: usize = 1_000;
+
+/// Hard global pinned ceiling (D8 / gc.md §2).
+pub(super) const MAX_PINNED_TOTAL: usize = 20_000;
+
+/// Maximum provenance entries kept per event.
+pub(super) const MAX_PROVENANCE_ENTRIES: usize = 32;
+
+/// Tombstones older than this many seconds are purged by `gc_step`.
+pub(super) const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600; // 90 days
+
+// ─── Shared storage type ─────────────────────────────────────────────────────
+
+/// Shared storage map for a single domain namespace.
+type DomainMap = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;
+
+// ─── Inner state ─────────────────────────────────────────────────────────────
+
+pub(super) struct MemState {
+    /// Primary event store: hex id → StoredEvent.
+    pub(super) events: HashMap<String, StoredEvent>,
+
+    /// Tombstone rows: hex target_id → TombstoneRow.
+    pub(super) tombstones: HashMap<String, TombstoneRow>,
+
+    /// Address tombstones (kind:5 `a`-tag): "kind:pubkey:dtag" → TombstoneRow.
+    pub(super) addr_tombstones: HashMap<String, TombstoneRow>,
+
+    /// Provenance: hex event_id → sorted Vec<ProvenanceEntry>.
+    pub(super) provenance: HashMap<String, Vec<ProvenanceEntry>>,
+
+    /// Watermarks: (filter_hash_hex, relay_url) → WatermarkRow.
+    pub(super) watermarks: HashMap<(String, String), WatermarkRow>,
+
+    /// Domain data per namespace.
+    pub(super) domain_data: HashMap<&'static str, DomainMap>,
+
+    /// Domain schema versions.
+    pub(super) domain_versions: HashMap<&'static str, u32>,
+
+    /// Claim budgets: claimer → max pinned.
+    pub(super) claim_budgets: HashMap<ClaimerId, usize>,
+
+    /// Current claims: claimer → BTreeSet of hex event ids.
+    /// BTreeSet gives idempotency per T25 — re-claiming a known id is a no-op.
+    pub(super) claims: HashMap<ClaimerId, BTreeSet<String>>,
+}
+
+impl MemState {
+    pub(super) fn new() -> Self {
+        Self {
+            events: HashMap::new(),
+            tombstones: HashMap::new(),
+            addr_tombstones: HashMap::new(),
+            provenance: HashMap::new(),
+            watermarks: HashMap::new(),
+            domain_data: HashMap::new(),
+            domain_versions: HashMap::new(),
+            claim_budgets: HashMap::new(),
+            claims: HashMap::new(),
+        }
+    }
+
+    #[allow(dead_code)] // Available for future dump/debug helpers.
+    pub(super) fn events_sorted_newest_first(&self) -> Vec<&StoredEvent> {
+        let mut v: Vec<&StoredEvent> = self.events.values().collect();
+        v.sort_by(|a, b| {
+            b.raw.created_at
+                .cmp(&a.raw.created_at)
+                .then(a.raw.id.cmp(&b.raw.id))
+        });
+        v
+    }
+}
+
+// ─── MemEventStore ───────────────────────────────────────────────────────────
+
+/// Fully in-memory `EventStore` implementation.
+pub struct MemEventStore {
+    pub(super) state: Mutex<MemState>,
+}
+
+impl MemEventStore {
+    pub fn new() -> Self {
+        Self {
+            state: Mutex::new(MemState::new()),
+        }
+    }
+
+    pub(super) fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemState>, StoreError> {
+        self.state.lock().map_err(|e| StoreError::Io(e.to_string()))
+    }
+}
+
+impl Default for MemEventStore {
+    fn default() -> Self {
+        Self::new()
+    }
+}
+
+// ─── Provenance helpers ──────────────────────────────────────────────────────
+
+pub(super) fn sort_provenance(entries: &mut [ProvenanceEntry]) {
+    entries.sort_by(|a, b| {
+        a.first_seen_ms
+            .cmp(&b.first_seen_ms)
+            .then(a.relay_url.cmp(&b.relay_url))
+    });
+    for (i, e) in entries.iter_mut().enumerate() {
+        e.primary = i == 0;
+    }
+}
+
+pub(super) fn upsert_provenance(
+    entries: &mut Vec<ProvenanceEntry>,
+    relay_url: RelayUrl,
+    received_at_ms: u64,
+) {
+    // Update existing entry if present.
+    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
+        if received_at_ms < e.first_seen_ms {
+            e.first_seen_ms = received_at_ms;
+        }
+        if received_at_ms > e.last_seen_ms {
+            e.last_seen_ms = received_at_ms;
+        }
+        sort_provenance(entries);
+        return;
+    }
+
+    // If at capacity, overwrite the oldest non-primary entry.
+    if entries.len() >= MAX_PROVENANCE_ENTRIES {
+        if let Some(oldest) = entries.iter_mut().skip(1).min_by_key(|e| e.last_seen_ms) {
+            *oldest = ProvenanceEntry {
+                relay_url,
+                first_seen_ms: received_at_ms,
+                last_seen_ms: received_at_ms,
+                primary: false,
+            };
+            sort_provenance(entries);
+            return;
+        }
+    }
+
+    entries.push(ProvenanceEntry {
+        relay_url,
+        first_seen_ms: received_at_ms,
+        last_seen_ms: received_at_ms,
+        primary: false,
+    });
+    sort_provenance(entries);
+}
+
+// ─── Hex utilities ───────────────────────────────────────────────────────────
+
+pub(super) fn bytes_to_hex(b: &[u8]) -> String {
+    b.iter().map(|byte| format!("{byte:02x}")).collect()
+}
diff --git a/crates/nmp-core/src/store/mem/query.rs b/crates/nmp-core/src/store/mem/query.rs
new file mode 100644
index 0000000..615bc71
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/query.rs
@@ -0,0 +1,394 @@
+//! Read / scan / watermark / dump methods for `MemEventStore`.
+//!
+//! These are pure reads; all state mutation lives in `insert.rs` and `gc.rs`.
+
+use super::{bytes_to_hex, MemEventStore};
+use crate::store::events::EventIter;
+use crate::store::types::{
+    Coverage, DumpFormat, DumpStats, EventId, ProvenanceEntry, PubKey, StoredEvent,
+    TombstoneRow, WatermarkKey, WatermarkRow,
+};
+use crate::store::StoreError;
+
+// ─── Primary lookups ─────────────────────────────────────────────────────────
+
+pub(super) fn get_by_id(
+    store: &MemEventStore,
+    id: &EventId,
+) -> Result<Option<StoredEvent>, StoreError> {
+    let hex = bytes_to_hex(id);
+    let st = store.lock()?;
+    Ok(st.events.get(&hex).cloned())
+}
+
+pub(super) fn scan_by_author_kind<'a>(
+    store: &'a MemEventStore,
+    author: &PubKey,
+    kinds: &[u32],
+    since: Option<u64>,
+    until: Option<u64>,
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let author_hex = bytes_to_hex(author);
+    let st = store.lock()?;
+    let mut results: Vec<StoredEvent> = st
+        .events
+        .values()
+        .filter(|ev| {
+            ev.raw.pubkey == author_hex
+                && kinds.contains(&ev.raw.kind)
+                && since.is_none_or(|s| ev.raw.created_at >= s)
+                && until.is_none_or(|u| ev.raw.created_at <= u)
+        })
+        .cloned()
+        .collect();
+    results.sort_by(|a, b| {
+        b.raw.created_at
+            .cmp(&a.raw.created_at)
+            .then(a.raw.id.cmp(&b.raw.id))
+    });
+    results.truncate(limit);
+    Ok(Box::new(results.into_iter().map(Ok)))
+}
+
+pub(super) fn get_param_replaceable(
+    store: &MemEventStore,
+    pubkey: &PubKey,
+    kind: u32,
+    d_tag: &[u8],
+) -> Result<Option<StoredEvent>, StoreError> {
+    let pubkey_hex = bytes_to_hex(pubkey);
+    let d_str = String::from_utf8_lossy(d_tag).into_owned();
+    let st = store.lock()?;
+    let winner = st
+        .events
+        .values()
+        .filter(|ev| {
+            ev.raw.pubkey == pubkey_hex
+                && ev.raw.kind == kind
+                && ev.raw
+                    .d_tag()
+                    .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
+                    .unwrap_or(false)
+        })
+        .max_by(|a, b| {
+            a.raw.created_at
+                .cmp(&b.raw.created_at)
+                .then(b.raw.id.cmp(&a.raw.id))
+        })
+        .cloned();
+    Ok(winner)
+}
+
+pub(super) fn scan_by_kind_dtag<'a>(
+    store: &'a MemEventStore,
+    kind: u32,
+    d_tag: &[u8],
+    since: Option<u64>,
+    until: Option<u64>,
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let d_str = String::from_utf8_lossy(d_tag).into_owned();
+    let st = store.lock()?;
+    let mut results: Vec<StoredEvent> = st
+        .events
+        .values()
+        .filter(|ev| {
+            ev.raw.kind == kind
+                && ev.raw
+                    .d_tag()
+                    .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
+                    .unwrap_or(false)
+                && since.is_none_or(|s| ev.raw.created_at >= s)
+                && until.is_none_or(|u| ev.raw.created_at <= u)
+        })
+        .cloned()
+        .collect();
+    results.sort_by(|a, b| {
+        b.raw.created_at
+            .cmp(&a.raw.created_at)
+            .then(a.raw.id.cmp(&b.raw.id))
+    });
+    results.truncate(limit);
+    Ok(Box::new(results.into_iter().map(Ok)))
+}
+
+pub(super) fn scan_by_etag<'a>(
+    store: &'a MemEventStore,
+    target: &EventId,
+    kinds: &[u32],
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let target_hex = bytes_to_hex(target);
+    let st = store.lock()?;
+    let mut results: Vec<StoredEvent> = st
+        .events
+        .values()
+        .filter(|ev| {
+            kinds.contains(&ev.raw.kind) && ev.raw.e_tags().contains(&target_hex)
+        })
+        .cloned()
+        .collect();
+    results.sort_by(|a, b| {
+        b.raw.created_at
+            .cmp(&a.raw.created_at)
+            .then(a.raw.id.cmp(&b.raw.id))
+    });
+    results.truncate(limit);
+    Ok(Box::new(results.into_iter().map(Ok)))
+}
+
+pub(super) fn scan_by_ptag<'a>(
+    store: &'a MemEventStore,
+    target: &PubKey,
+    kinds: &[u32],
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let target_hex = bytes_to_hex(target);
+    let st = store.lock()?;
+    let mut results: Vec<StoredEvent> = st
+        .events
+        .values()
+        .filter(|ev| {
+            kinds.contains(&ev.raw.kind) && ev.raw.p_tags().contains(&target_hex)
+        })
+        .cloned()
+        .collect();
+    results.sort_by(|a, b| {
+        b.raw.created_at
+            .cmp(&a.raw.created_at)
+            .then(a.raw.id.cmp(&b.raw.id))
+    });
+    results.truncate(limit);
+    Ok(Box::new(results.into_iter().map(Ok)))
+}
+
+pub(super) fn scan_by_kind_time<'a>(
+    store: &'a MemEventStore,
+    kinds: &[u32],
+    since: Option<u64>,
+    until: Option<u64>,
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let st = store.lock()?;
+    let mut results: Vec<StoredEvent> = st
+        .events
+        .values()
+        .filter(|ev| {
+            (kinds.is_empty() || kinds.contains(&ev.raw.kind))
+                && since.is_none_or(|s| ev.raw.created_at >= s)
+                && until.is_none_or(|u| ev.raw.created_at <= u)
+        })
+        .cloned()
+        .collect();
+    results.sort_by(|a, b| {
+        b.raw.created_at
+            .cmp(&a.raw.created_at)
+            .then(a.raw.id.cmp(&b.raw.id))
+    });
+    results.truncate(limit);
+    Ok(Box::new(results.into_iter().map(Ok)))
+}
+
+pub(super) fn scan_expiring_before<'a>(
+    store: &'a MemEventStore,
+    unix_seconds: u64,
+    limit: usize,
+) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+    let st = store.lock()?;
+    // Ascending by expiration.
+    let mut pairs: Vec<(u64, StoredEvent)> = st
+        .events
+        .values()
+        .filter_map(|ev| {
+            ev.raw
+                .expiration()
+                .filter(|&exp| exp < unix_seconds)
+                .map(|exp| (exp, ev.clone()))
+        })
+        .collect();
+    pairs.sort_by_key(|(exp, _)| *exp);
+    pairs.truncate(limit);
+    Ok(Box::new(pairs.into_iter().map(|(_, ev)| Ok(ev))))
+}
+
+pub(super) fn tombstones_for(
+    store: &MemEventStore,
+    target: &EventId,
+) -> Result<Vec<TombstoneRow>, StoreError> {
+    let hex = bytes_to_hex(target);
+    let st = store.lock()?;
+    Ok(st.tombstones.get(&hex).cloned().into_iter().collect())
+}
+
+pub(super) fn list_tombstones<'a>(
+    store: &'a MemEventStore,
+) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError> {
+    let st = store.lock()?;
+    let rows: Vec<TombstoneRow> = st.tombstones.values().cloned().collect();
+    Ok(Box::new(rows.into_iter().map(Ok)))
+}
+
+pub(super) fn provenance_for(
+    store: &MemEventStore,
+    id: &EventId,
+) -> Result<Vec<ProvenanceEntry>, StoreError> {
+    let hex = bytes_to_hex(id);
+    let st = store.lock()?;
+    Ok(st.provenance.get(&hex).cloned().unwrap_or_default())
+}
+
+// ─── Watermarks ──────────────────────────────────────────────────────────────
+
+pub(super) fn read_watermark(
+    store: &MemEventStore,
+    key: &WatermarkKey,
+) -> Result<Option<WatermarkRow>, StoreError> {
+    let st = store.lock()?;
+    let wm_key = (
+        bytes_to_hex(&key.filter_hash),
+        key.relay_url.clone(),
+    );
+    Ok(st.watermarks.get(&wm_key).cloned())
+}
+
+pub(super) fn write_watermark(
+    store: &MemEventStore,
+    row: WatermarkRow,
+) -> Result<(), StoreError> {
+    let mut st = store.lock()?;
+    let wm_key = (bytes_to_hex(&row.key.filter_hash), row.key.relay_url.clone());
+    st.watermarks.insert(wm_key, row);
+    Ok(())
+}
+
+pub(super) fn coverage(
+    store: &MemEventStore,
+    key: &WatermarkKey,
+) -> Result<Coverage, StoreError> {
+    let row = read_watermark(store, key)?;
+    let Some(row) = row else {
+        return Ok(Coverage::Unknown);
+    };
+    // Default staleness window: 300 seconds.
+    let staleness_window = 300u64;
+    let now = std::time::SystemTime::now()
+        .duration_since(std::time::UNIX_EPOCH)
+        .map(|d| d.as_secs())
+        .unwrap_or(0);
+    let age = now.saturating_sub(row.updated_at);
+    if age <= staleness_window {
+        Ok(Coverage::CompleteAsOf(row.synced_up_to))
+    } else {
+        Ok(Coverage::PartialUpTo(row.synced_up_to))
+    }
+}
+
+pub(super) fn list_watermarks_for_relay<'a>(
+    store: &'a MemEventStore,
+    relay_url: &str,
+) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
+{
+    let st = store.lock()?;
+    let rows: Vec<WatermarkRow> = st
+        .watermarks
+        .values()
+        .filter(|r| r.key.relay_url == relay_url)
+        .cloned()
+        .collect();
+    Ok(Box::new(rows.into_iter().map(Ok)))
+}
+
+// ─── Dump ────────────────────────────────────────────────────────────────────
+
+pub(super) fn dump(
+    store: &MemEventStore,
+    out: &mut dyn std::io::Write,
+    format: DumpFormat,
+) -> Result<DumpStats, StoreError> {
+    if !matches!(format, DumpFormat::Jsonl) {
+        return Err(StoreError::Io("CBOR dump not yet implemented".into()));
+    }
+
+    let st = store.lock()?;
+    let mut stats = DumpStats::default();
+
+    // Dump events in deterministic order (ascending hex id).
+    let mut event_ids: Vec<&String> = st.events.keys().collect();
+    event_ids.sort();
+    for id in event_ids {
+        let ev = &st.events[id];
+        let line = serde_json::json!({
+            "type": "event",
+            "event": *ev.raw,
+            "received_at_ms": ev.received_at_ms,
+        })
+        .to_string();
+        let bytes = (line + "\n").into_bytes();
+        stats.bytes_written += bytes.len() as u64;
+        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
+        stats.events += 1;
+    }
+
+    // Dump tombstones in deterministic order.
+    let mut tomb_ids: Vec<&String> = st.tombstones.keys().collect();
+    tomb_ids.sort();
+    for id in tomb_ids {
+        let t = &st.tombstones[id];
+        let line = serde_json::json!({
+            "type": "tombstone",
+            "target_id": bytes_to_hex(&t.target_id),
+            "deleted_at": t.deleted_at,
+            "origin": format!("{:?}", t.origin),
+        })
+        .to_string();
+        let bytes = (line + "\n").into_bytes();
+        stats.bytes_written += bytes.len() as u64;
+        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
+        stats.tombstones += 1;
+    }
+
+    // Dump watermarks in deterministic order.
+    let mut wm_keys: Vec<&(String, String)> = st.watermarks.keys().collect();
+    wm_keys.sort();
+    for k in wm_keys {
+        let r = &st.watermarks[k];
+        let line = serde_json::json!({
+            "type": "watermark",
+            "filter_hash": &r.key.filter_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
+            "relay_url": &r.key.relay_url,
+            "synced_up_to": r.synced_up_to,
+        })
+        .to_string();
+        let bytes = (line + "\n").into_bytes();
+        stats.bytes_written += bytes.len() as u64;
+        out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
+        stats.watermarks += 1;
+    }
+
+    // Dump domain rows in deterministic order (namespace, key).
+    let mut ns_list: Vec<&&'static str> = st.domain_data.keys().collect();
+    ns_list.sort();
+    for ns in ns_list {
+        let data = st.domain_data[ns]
+            .lock()
+            .map_err(|e| StoreError::Io(e.to_string()))?;
+        let mut pairs: Vec<(&Vec<u8>, &Vec<u8>)> = data.iter().collect();
+        pairs.sort_by_key(|(k, _)| *k);
+        for (k, v) in pairs {
+            let line = serde_json::json!({
+                "type": "domain",
+                "namespace": ns,
+                "key": k,
+                "value": v,
+            })
+            .to_string();
+            let bytes = (line + "\n").into_bytes();
+            stats.bytes_written += bytes.len() as u64;
+            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
+            stats.domain_rows += 1;
+        }
+    }
+
+    Ok(stats)
+}
diff --git a/crates/nmp-core/src/store/mem/store_impl.rs b/crates/nmp-core/src/store/mem/store_impl.rs
new file mode 100644
index 0000000..78d83f1
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/store_impl.rs
@@ -0,0 +1,181 @@
+//! `EventStore` trait implementation for `MemEventStore`.
+//!
+//! Pure delegation — all logic lives in the sub-modules. This file exists so
+//! `mod.rs` stays under 200 LOC (Article I hard ceiling).
+
+use super::{domain, gc, insert, query, MemEventStore};
+use crate::store::events::{DomainHandle, EventIter, EventStore};
+use crate::store::types::{
+    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
+    InsertOutcome, ProvenanceEntry, PubKey, RelayUrl, StoredEvent,
+    TombstoneRow, VerifiedEvent, WatermarkKey, WatermarkRow,
+};
+use crate::store::StoreError;
+use crate::substrate::DomainMigration;
+
+impl EventStore for MemEventStore {
+    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
+        query::get_by_id(self, id)
+    }
+
+    fn scan_by_author_kind<'a>(
+        &'a self,
+        author: &PubKey,
+        kinds: &[u32],
+        since: Option<u64>,
+        until: Option<u64>,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_by_author_kind(self, author, kinds, since, until, limit)
+    }
+
+    fn get_param_replaceable(
+        &self,
+        pubkey: &PubKey,
+        kind: u32,
+        d_tag: &[u8],
+    ) -> Result<Option<StoredEvent>, StoreError> {
+        query::get_param_replaceable(self, pubkey, kind, d_tag)
+    }
+
+    fn scan_by_kind_dtag<'a>(
+        &'a self,
+        kind: u32,
+        d_tag: &[u8],
+        since: Option<u64>,
+        until: Option<u64>,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_by_kind_dtag(self, kind, d_tag, since, until, limit)
+    }
+
+    fn scan_by_etag<'a>(
+        &'a self,
+        target: &EventId,
+        kinds: &[u32],
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_by_etag(self, target, kinds, limit)
+    }
+
+    fn scan_by_ptag<'a>(
+        &'a self,
+        target: &PubKey,
+        kinds: &[u32],
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_by_ptag(self, target, kinds, limit)
+    }
+
+    fn scan_by_kind_time<'a>(
+        &'a self,
+        kinds: &[u32],
+        since: Option<u64>,
+        until: Option<u64>,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_by_kind_time(self, kinds, since, until, limit)
+    }
+
+    fn scan_expiring_before<'a>(
+        &'a self,
+        unix_seconds: u64,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
+        query::scan_expiring_before(self, unix_seconds, limit)
+    }
+
+    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
+        query::tombstones_for(self, target)
+    }
+
+    fn list_tombstones<'a>(
+        &'a self,
+    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
+    {
+        query::list_tombstones(self)
+    }
+
+    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
+        query::provenance_for(self, id)
+    }
+
+    fn insert(
+        &self,
+        event: VerifiedEvent,
+        source: &RelayUrl,
+        received_at_ms: u64,
+    ) -> Result<InsertOutcome, StoreError> {
+        insert::insert(self, event.into_raw(), source, received_at_ms)
+    }
+
+    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
+        insert::delete_by_filter(self, filter)
+    }
+
+    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
+        query::read_watermark(self, key)
+    }
+
+    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError> {
+        query::write_watermark(self, row)
+    }
+
+    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError> {
+        query::coverage(self, key)
+    }
+
+    fn list_watermarks_for_relay<'a>(
+        &'a self,
+        relay_url: &str,
+    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
+    {
+        query::list_watermarks_for_relay(self, relay_url)
+    }
+
+    fn register_view_cover(
+        &self,
+        claimer: ClaimerId,
+        cover_budget: usize,
+    ) -> Result<(), StoreError> {
+        gc::register_view_cover(self, claimer, cover_budget)
+    }
+
+    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
+        gc::claim(self, claimer, ids)
+    }
+
+    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
+        gc::release(self, claimer)
+    }
+
+    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
+        // Memory backend has no LRU — all events are equally hot. No-op.
+        Ok(())
+    }
+
+    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
+        gc::gc_step(self, budget)
+    }
+
+    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
+        domain::domain_open(self, namespace)
+    }
+
+    fn run_migrations(
+        &self,
+        namespace: &'static str,
+        target_version: u32,
+        migrations: &[DomainMigration],
+    ) -> Result<(), StoreError> {
+        domain::run_migrations(self, namespace, target_version, migrations)
+    }
+
+    fn dump(
+        &self,
+        out: &mut dyn std::io::Write,
+        format: DumpFormat,
+    ) -> Result<DumpStats, StoreError> {
+        query::dump(self, out, format)
+    }
+}
diff --git a/crates/nmp-core/src/store/mem/tests.rs b/crates/nmp-core/src/store/mem/tests.rs
new file mode 100644
index 0000000..6301495
--- /dev/null
+++ b/crates/nmp-core/src/store/mem/tests.rs
@@ -0,0 +1,132 @@
+//! Unit tests for `MemEventStore` — P2 invariant checks.
+//!
+//! Integration tests using the full `StoreHarness` live in
+//! `crates/nmp-testing/tests/store_*.rs`.
+
+#[cfg(test)]
+mod insert_tests {
+    use crate::store::types::{InsertOutcome, RawEvent, VerifiedEvent};
+    use crate::store::{EventStore, MemEventStore};
+
+    fn unchecked(raw: RawEvent) -> VerifiedEvent {
+        VerifiedEvent::from_raw_unchecked(raw)
+    }
+
+    /// P2: tombstone upsert must max-merge `deleted_at` and union sources.
+    #[test]
+    fn tombstone_max_merge_takes_newer_deleted_at() {
+        let store = MemEventStore::new();
+        let target_hex = "0a".repeat(32);
+        let k5a_hex = "a1".repeat(32);
+        let k5b_hex = "b2".repeat(32);
+
+        // First kind:5 at t=100.
+        let k5a = RawEvent {
+            id: k5a_hex.clone(),
+            pubkey: "01".repeat(32),
+            created_at: 100,
+            kind: 5,
+            tags: vec![vec!["e".into(), target_hex.clone()]],
+            content: String::new(),
+            sig: "a".repeat(128),
+        };
+        store.insert(unchecked(k5a), &"wss://r1/".to_string(), 100_000).unwrap();
+
+        // Second kind:5 at t=200 (newer — should win for deleted_at).
+        let k5b = RawEvent {
+            id: k5b_hex.clone(),
+            pubkey: "01".repeat(32),
+            created_at: 200,
+            kind: 5,
+            tags: vec![vec!["e".into(), target_hex.clone()]],
+            content: String::new(),
+            sig: "a".repeat(128),
+        };
+        store.insert(unchecked(k5b), &"wss://r2/".to_string(), 200_000).unwrap();
+
+        let st = store.state.lock().unwrap();
+        let tomb = st.tombstones.get(&target_hex).expect("tombstone must exist");
+        assert_eq!(tomb.deleted_at, 200, "max-merge must take the newer deleted_at");
+        assert!(tomb.sources.contains(&"wss://r1/".to_string()), "must union r1");
+        assert!(tomb.sources.contains(&"wss://r2/".to_string()), "must union r2");
+    }
+
+    /// P2: same-id re-delivery for replaceable events must merge provenance,
+    /// not count as a new supersession.
+    #[test]
+    fn replaceable_dup_id_merges_provenance() {
+        let store = MemEventStore::new();
+        let pk = "01".repeat(32);
+        let id = "aa".repeat(32);
+        let ev = RawEvent {
+            id: id.clone(),
+            pubkey: pk.clone(),
+            created_at: 1000,
+            kind: 0, // replaceable
+            tags: vec![],
+            content: String::new(),
+            sig: "a".repeat(128),
+        };
+
+        let o1 = store.insert(unchecked(ev.clone()), &"wss://r1/".to_string(), 1_000_000).unwrap();
+        assert!(matches!(o1, InsertOutcome::Inserted { .. }));
+
+        let o2 = store.insert(unchecked(ev), &"wss://r2/".to_string(), 2_000_000).unwrap();
+        assert!(
+            matches!(o2, InsertOutcome::Duplicate { .. }),
+            "re-delivery of same id must be Duplicate, got {o2:?}"
+        );
+
+        let id_bytes = [0xaau8; 32];
+        let prov = store.provenance_for(&id_bytes).unwrap();
+        assert_eq!(prov.len(), 2, "both relays must be in provenance");
+    }
+}
+
+#[cfg(test)]
+mod gc_tests {
+    use crate::store::types::ClaimerId;
+    use crate::store::{EventStore, MemEventStore, StoreError};
+
+    fn make_id(b: u8) -> [u8; 32] {
+        let mut id = [0u8; 32];
+        id[0] = b;
+        id
+    }
+
+    #[test]
+    fn claim_idempotent_reclaim_does_not_count() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(1);
+        store.register_view_cover(c, 5).unwrap();
+        let id = make_id(1);
+        store.claim(c, &[id]).unwrap();
+        store.claim(c, &[id]).unwrap();
+        let st = store.state.lock().unwrap();
+        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
+    }
+
+    #[test]
+    fn claim_over_per_view_ceiling_returns_err() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(2);
+        store.register_view_cover(c, 2).unwrap();
+        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
+        let result = store.claim(c, &[make_id(3)]);
+        assert!(
+            matches!(result, Err(StoreError::OverPinned { .. })),
+            "must return OverPinned when per-view ceiling exceeded"
+        );
+    }
+
+    #[test]
+    fn release_clears_all_pins() {
+        let store = MemEventStore::new();
+        let c = ClaimerId(3);
+        store.register_view_cover(c, 100).unwrap();
+        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
+        store.release(c).unwrap();
+        let st = store.state.lock().unwrap();
+        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
+    }
+}
diff --git a/crates/nmp-core/src/store/mod.rs b/crates/nmp-core/src/store/mod.rs
index 12eb8e8..8906df0 100644
--- a/crates/nmp-core/src/store/mod.rs
+++ b/crates/nmp-core/src/store/mod.rs
@@ -25,11 +25,11 @@ pub use mem::MemEventStore;
 pub use types::{
     ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
     InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RejectReason, RelayUrl, StoredEvent,
-    SyncMethod, TombstoneOrigin, TombstoneRow, WatermarkKey, WatermarkRow,
+    SyncMethod, TombstoneOrigin, TombstoneRow, VerifiedEvent, WatermarkKey, WatermarkRow,
 };
 
-// Re-export StoreError from types (defined there to avoid circular imports).
-pub use types::StoreError;
+// Re-export error types from types (defined there to avoid circular imports).
+pub use types::{StoreError, VerifyError};
 
 use std::path::PathBuf;
 
diff --git a/crates/nmp-core/src/store/types.rs b/crates/nmp-core/src/store/types.rs
deleted file mode 100644
index efb9ee6..0000000
--- a/crates/nmp-core/src/store/types.rs
+++ /dev/null
@@ -1,343 +0,0 @@
-//! Supporting types for `EventStore`.
-//!
-//! These types live here and are re-exported from `nmp_core::store`.
-//! They track the design in `docs/design/lmdb/trait/types.md`.
-//!
-//! NOTE: The design references `nostr::Event` / `nostr::Keys` from the upstream nostr crate.
-//! Since that crate is not yet in the workspace, this module uses `RawEvent` as a temporary
-//! stand-in. Full signature verification is deferred to the M3-lmdb follow-up task.
-
-use serde::{Deserialize, Serialize};
-use std::sync::Arc;
-
-// ─── Type aliases ────────────────────────────────────────────────────────────
-
-pub type EventId = [u8; 32];
-pub type PubKey = [u8; 32];
-pub type RelayUrl = String;
-
-// ─── RawEvent (stand-in for nostr::Event) ────────────────────────────────────
-
-/// Temporary stand-in for `nostr::Event` until the nostr crate is in the workspace.
-///
-/// Fields match the NIP-01 event object exactly. Signature verification is
-/// skipped for now (insert always trusts the caller). The M3-lmdb task will
-/// swap this for the real type and enable proper sig checks.
-#[derive(Clone, Debug, Serialize, Deserialize)]
-pub struct RawEvent {
-    pub id: String,          // lowercase hex
-    pub pubkey: String,      // lowercase hex
-    pub created_at: u64,     // unix seconds
-    pub kind: u32,
-    pub tags: Vec<Vec<String>>,
-    pub content: String,
-    pub sig: String,         // lowercase hex
-}
-
-impl RawEvent {
-    /// Decode hex id → 32 bytes. Returns zeroes on malformed input.
-    pub fn id_bytes(&self) -> EventId {
-        hex_to_bytes32(&self.id)
-    }
-
-    /// Decode hex pubkey → 32 bytes. Returns zeroes on malformed input.
-    pub fn pubkey_bytes(&self) -> PubKey {
-        hex_to_bytes32(&self.pubkey)
-    }
-
-    /// NIP-01 replaceable kinds: 0, 3, and 10000–19999.
-    pub fn is_replaceable(&self) -> bool {
-        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
-    }
-
-    /// NIP-33 parameterized replaceable kinds: 30000–39999.
-    pub fn is_param_replaceable(&self) -> bool {
-        (30_000..40_000).contains(&self.kind)
-    }
-
-    /// NIP-16 ephemeral kinds: 20000–29999.
-    pub fn is_ephemeral(&self) -> bool {
-        (20_000..30_000).contains(&self.kind)
-    }
-
-    /// Returns the value of the first `d` tag, if present.
-    pub fn d_tag(&self) -> Option<Vec<u8>> {
-        self.tags.iter().find(|t| t.first().map(|s| s == "d").unwrap_or(false))
-            .and_then(|t| t.get(1))
-            .map(|s| s.as_bytes().to_vec())
-    }
-
-    /// Returns the unix-second value of the first `expiration` tag, if present.
-    pub fn expiration(&self) -> Option<u64> {
-        self.tags.iter()
-            .find(|t| t.first().map(|s| s == "expiration").unwrap_or(false))
-            .and_then(|t| t.get(1))
-            .and_then(|s| s.parse::<u64>().ok())
-    }
-
-    /// Returns all `e`-tag target ids (lowercase hex).
-    pub fn e_tags(&self) -> Vec<String> {
-        self.tags.iter()
-            .filter(|t| t.first().map(|s| s == "e").unwrap_or(false))
-            .filter_map(|t| t.get(1).cloned())
-            .collect()
-    }
-
-    /// Returns all `p`-tag target pubkeys (lowercase hex).
-    pub fn p_tags(&self) -> Vec<String> {
-        self.tags.iter()
-            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
-            .filter_map(|t| t.get(1).cloned())
-            .collect()
-    }
-
-    /// Returns all `a`-tag target addresses (e.g. "30023:pubkey:dtag").
-    pub fn a_tags(&self) -> Vec<String> {
-        self.tags.iter()
-            .filter(|t| t.first().map(|s| s == "a").unwrap_or(false))
-            .filter_map(|t| t.get(1).cloned())
-            .collect()
-    }
-
-    /// Validates the event has a plausible structure (non-empty id, pubkey, sig).
-    /// Full cryptographic verification is deferred until the nostr crate is wired in.
-    pub fn is_structurally_valid(&self) -> bool {
-        self.id.len() == 64 && self.pubkey.len() == 64 && self.sig.len() == 128
-    }
-}
-
-fn hex_to_bytes32(s: &str) -> [u8; 32] {
-    let mut out = [0u8; 32];
-    if s.len() != 64 {
-        return out;
-    }
-    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
-        if i >= 32 { break; }
-        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
-            out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
-        }
-    }
-    out
-}
-
-fn hex_nibble(b: u8) -> u8 {
-    match b {
-        b'0'..=b'9' => b - b'0',
-        b'a'..=b'f' => b - b'a' + 10,
-        b'A'..=b'F' => b - b'A' + 10,
-        _ => 0,
-    }
-}
-
-// ─── StoredEvent ──────────────────────────────────────────────────────────────
-
-/// A stored Nostr event with arrival metadata.
-///
-/// `raw` is `Arc<RawEvent>` so the hot LRU can hold reference-counted copies
-/// without cloning the event body on each `get_by_id`.
-#[derive(Clone, Debug)]
-pub struct StoredEvent {
-    pub raw: Arc<RawEvent>,
-    pub received_at_ms: u64,   // wall-clock first arrival across all relays
-}
-
-// ─── Insert outcomes ──────────────────────────────────────────────────────────
-
-#[derive(Clone, Debug)]
-pub enum InsertOutcome {
-    /// Fresh insert; secondary indexes written.
-    Inserted { id: EventId, sources_after: u32 },
-    /// Duplicate id; provenance updated, primary untouched.
-    Duplicate { id: EventId, sources_after: u32 },
-    /// Replaceable supersession: this event replaced an older one.
-    Replaced { new_id: EventId, replaced_id: EventId },
-    /// Replaceable supersession: incoming was older, dropped.
-    Superseded { id: EventId, current_id: EventId },
-    /// Suppressed because a tombstone exists for this event id.
-    Tombstoned { id: EventId, kind5_event_id: Option<EventId>, origin: TombstoneOrigin },
-    /// Signature / delegation / structural validity failed.
-    Rejected { id: EventId, reason: RejectReason },
-    /// Ephemeral kind: delivered to live consumers, not stored.
-    Ephemeral { id: EventId },
-}
-
-#[derive(Clone, Debug)]
-pub enum RejectReason {
-    BadSignature,
-    BadDelegation(String),
-    Malformed(String),
-    /// NIP-40 expiration already in the past.
-    ExpiredOnArrival,
-}
-
-// ─── Tombstones ───────────────────────────────────────────────────────────────
-
-#[derive(Clone, Debug)]
-pub struct TombstoneRow {
-    pub target_id: EventId,
-    /// None for NIP-40 expiry and AdminPurge tombstones.
-    pub kind5_event_id: Option<EventId>,
-    /// None for NIP40Expiry / AdminPurge.
-    pub deleter_pubkey: Option<PubKey>,
-    /// Unix seconds; max observed across redeliveries.
-    pub deleted_at: u64,
-    pub sources: Vec<RelayUrl>,
-    pub origin: TombstoneOrigin,
-}
-
-#[derive(Clone, Copy, Debug, Eq, PartialEq)]
-pub enum TombstoneOrigin {
-    Kind5,
-    NIP40Expiry,
-    AdminPurge,
-}
-
-// ─── Provenance ───────────────────────────────────────────────────────────────
-
-#[derive(Clone, Debug)]
-pub struct ProvenanceEntry {
-    pub relay_url: RelayUrl,
-    pub first_seen_ms: u64,
-    pub last_seen_ms: u64,
-    /// True for the first relay that delivered this event (deterministic after sort).
-    pub primary: bool,
-}
-
-// ─── Watermarks ───────────────────────────────────────────────────────────────
-
-#[derive(Clone, Debug, PartialEq)]
-pub struct WatermarkKey {
-    pub filter_hash: [u8; 32],
-    pub relay_url: RelayUrl,
-}
-
-#[derive(Clone, Debug)]
-pub struct WatermarkRow {
-    pub key: WatermarkKey,
-    pub synced_up_to: u64,    // unix seconds
-    pub last_sync_method: SyncMethod,
-    /// Engine-opaque resume blob (M4).
-    pub last_negentropy_state: Option<Vec<u8>>,
-    pub bytes_saved_vs_req: u64,
-    pub updated_at: u64,
-}
-
-#[derive(Clone, Copy, Debug, Eq, PartialEq)]
-pub enum SyncMethod {
-    Negentropy,
-    ReqScan,
-    Manual,
-}
-
-/// Returned by `coverage()` to classify watermark freshness.
-#[derive(Clone, Copy, Debug, Eq, PartialEq)]
-pub enum Coverage {
-    /// Fully synced; a cache miss is authoritative "doesn't exist".
-    CompleteAsOf(u64),
-    /// Synced up to timestamp but row is stale — fetch is needed.
-    PartialUpTo(u64),
-    /// No watermark; always fetch.
-    Unknown,
-}
-
-// ─── GC / hot-set ─────────────────────────────────────────────────────────────
-
-/// Opaque view-handle id assigned by the actor (monotonically increasing u64).
-#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
-pub struct ClaimerId(pub u64);
-
-/// Budget for one `gc_step()` call.
-#[derive(Clone, Copy, Debug)]
-pub struct GcBudget {
-    pub max_events_per_step: usize,
-    pub max_duration_ms: u32,
-}
-
-/// Report produced by `gc_step()`.
-#[derive(Clone, Debug, Default)]
-pub struct GcReport {
-    pub expired_reaped: usize,
-    pub lru_evicted: usize,
-    pub tombstones_purged: usize,
-    pub duration_ms: u32,
-}
-
-// ─── Filters ──────────────────────────────────────────────────────────────────
-
-/// NMP-internal delete filter — NOT a pass-through to nostr::Filter.
-/// Only exposes operations the kernel legitimately needs; does not allow
-/// arbitrary remote filters as a delete vector.
-#[derive(Clone, Debug)]
-pub enum DeleteFilter {
-    /// All events sourced exclusively from this relay.
-    ByRelayOnly(RelayUrl),
-    /// All events by a specific pubkey.
-    ByAuthor(PubKey),
-    /// Specific event ids.
-    ByIds(Vec<EventId>),
-    /// All events with kind in `[lo, hi]` (inclusive range).
-    ByKindRange { lo: u32, hi: u32 },
-}
-
-// ─── Export ───────────────────────────────────────────────────────────────────
-

=== T30 diff (cap 3000) ===
diff --git a/crates/nmp-core/src/kernel/requests/profile.rs b/crates/nmp-core/src/kernel/requests/profile.rs
index 28881cb..bee7cae 100644
--- a/crates/nmp-core/src/kernel/requests/profile.rs
+++ b/crates/nmp-core/src/kernel/requests/profile.rs
@@ -1,6 +1,6 @@
 //! Profile, author, and diagnostic-firehose request builders.
 //!
-//! # TODO(M2-migration)
+//! # M2 migration plan (compiler.md §3.5)
 //! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
 //! builders are scheduled for replacement by `SubscriptionCompiler`-driven
 //! interest registration once the wire-emitter, InterestRegistry, and
diff --git a/crates/nmp-core/src/kernel/requests/thread.rs b/crates/nmp-core/src/kernel/requests/thread.rs
index 25f5206..00a518d 100644
--- a/crates/nmp-core/src/kernel/requests/thread.rs
+++ b/crates/nmp-core/src/kernel/requests/thread.rs
@@ -1,6 +1,6 @@
 //! Thread view open/close/hydration request builders.
 //!
-//! # TODO(M2-migration)
+//! # M2 migration plan (compiler.md §3.5)
 //! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
 //! builders are scheduled for replacement by `SubscriptionCompiler`-driven
 //! interest registration once the wire-emitter, InterestRegistry, and
diff --git a/crates/nmp-core/src/planner/compiler.rs b/crates/nmp-core/src/planner/compiler.rs
deleted file mode 100644
index d5283df..0000000
--- a/crates/nmp-core/src/planner/compiler.rs
+++ /dev/null
@@ -1,498 +0,0 @@
-//! The subscription compiler: 4-stage pipeline from `Vec<LogicalInterest>`
-//! to `CompiledPlan`.
-//!
-//! ## Pipeline stages
-//!
-//! 1. **Resolve authors → mailboxes** — consult `MailboxCache` (stubbed in
-//!    phase 1 via `EmptyMailboxCache`; real impl lives in `nmp-nip65`).
-//! 2. **Indexer fallback** — authors with no known mailbox route to the
-//!    configured indexer set.
-//! 3. **Per-relay shape merge** — group by relay URL; merge compatible shapes
-//!    with `lattice::merge()` (Rules 1–8). Author sets are partitioned per
-//!    relay — only authors that declared a relay appear in its sub-shape.
-//! 4. **Plan-id binding** — hash(sorted interests, sorted mailbox snapshot,
-//!    lattice version) → stable `plan_id`.
-//!
-//! Design: `docs/design/subscription-compilation/compiler.md` §3
-//! Doctrine: D3 (outbox routing automatic), D6 (errors never cross FFI),
-//!           D8 (zero per-event allocs after warmup).
-//!
-//! Phase 1 scope: mailbox stage stubs to `EmptyMailboxCache`. Real mailbox
-//! resolution lives in `nmp-nip65` (separate later slice). The call-point
-//! for `EventStore::coverage()` is marked with a placeholder comment per the
-//! task spec (not wired in phase 1).
-
-use std::collections::{BTreeMap, BTreeSet, HashMap};
-
-use super::{
-    interest::{
-        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
-        RelayUrl,
-    },
-    lattice::{merge, MergeOutcome},
-    plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape},
-};
-
-// ─── MailboxCache seam ───────────────────────────────────────────────────────
-
-/// Minimal mailbox snapshot used by the compiler.
-///
-/// Phase 1: only `write_relays` and `both_relays` are consumed (Outbox
-/// direction). Inbox direction (read_relays) is used for `#p` interests, which
-/// are not yet wired in phase 1.
-///
-/// Full trait lives in `nmp-nip65::cache::MailboxCache` (later slice).
-#[derive(Clone, Debug, Default)]
-pub struct MailboxSnapshot {
-    pub write_relays: Vec<RelayUrl>,
-    pub read_relays: Vec<RelayUrl>,
-    pub both_relays: Vec<RelayUrl>,
-}
-
-impl MailboxSnapshot {
-    /// All relays relevant for Outbox direction (write + both).
-    pub fn outbox_relays(&self) -> impl Iterator<Item = &RelayUrl> {
-        self.write_relays.iter().chain(self.both_relays.iter())
-    }
-}
-
-/// Minimum surface the compiler needs for mailbox lookups.
-/// Phase 1 implementation: `EmptyMailboxCache` always returns `None`.
-/// Phase 2 implementation: `nmp-nip65::InMemoryMailboxCache`.
-pub trait MailboxCache: Send + Sync {
-    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
-    /// Snapshot of all known entries for plan-id hashing.
-    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
-    /// Monotonic generation counter — advances on every accepted `put`.
-    fn generation(&self) -> u64;
-}
-
-/// Phase 1 stub: no mailbox data. All authors fall back to the indexer set.
-pub struct EmptyMailboxCache;
-
-impl MailboxCache for EmptyMailboxCache {
-    fn get(&self, _pubkey: &Pubkey) -> Option<MailboxSnapshot> {
-        None
-    }
-    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
-        Vec::new()
-    }
-    fn generation(&self) -> u64 {
-        0
-    }
-}
-
-/// Simple in-memory mailbox cache for tests and the planner harness.
-#[derive(Default)]
-pub struct InMemoryMailboxCache {
-    data: HashMap<Pubkey, MailboxSnapshot>,
-    generation: u64,
-}
-
-impl InMemoryMailboxCache {
-    pub fn new() -> Self {
-        Self::default()
-    }
-
-    pub fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot) {
-        self.data.insert(pubkey, snapshot);
-        self.generation = self.generation.saturating_add(1);
-    }
-}
-
-impl MailboxCache for InMemoryMailboxCache {
-    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
-        self.data.get(pubkey).cloned()
-    }
-    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
-        self.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
-    }
-    fn generation(&self) -> u64 {
-        self.generation
-    }
-}
-
-// ─── Internal relay-partitioned entry ────────────────────────────────────────
-
-/// A relay-partitioned slice of one logical interest.
-///
-/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
-/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
-/// authors that declared this specific relay (not all N authors). This is the
-/// author-partitioning that lets the merge lattice produce per-relay author
-/// subsets.
-struct RelayEntry {
-    /// The interest's non-author fields (kinds, tags, since, until, etc.).
-    /// `authors` is intentionally left empty here; we merge `authors_for_relay`
-    /// in at Stage 3 merge time.
-    base_shape: InterestShape,
-    /// The subset of authors from this interest that declared this relay.
-    authors_for_relay: BTreeSet<Pubkey>,
-    /// Address-pointer coordinates from this interest (if relevant for routing).
-    addresses_for_relay: BTreeSet<NaddrCoord>,
-    lifecycle: InterestLifecycle,
-    source: RoutingSource,
-    interest_id: InterestId,
-}
-
-impl RelayEntry {
-    /// Construct the final `InterestShape` for this relay slice.
-    fn into_shape(mut self) -> (InterestShape, InterestLifecycle, RoutingSource, InterestId) {
-        self.base_shape.authors = self.authors_for_relay;
-        self.base_shape.addresses = self.addresses_for_relay;
-        (self.base_shape, self.lifecycle, self.source, self.interest_id)
-    }
-}
-
-// ─── SubscriptionCompiler ────────────────────────────────────────────────────
-
-/// Version of the merge lattice — bump when Rule semantics change.
-/// Included in plan-id hash to ensure plan-ids invalidate on lattice changes.
-const MERGE_LATTICE_VERSION: u8 = 1;
-
-/// The subscription compiler.
-///
-/// Holds a reference to the mailbox cache and indexer relay set. Both may be
-/// updated between compilations (the compiler always reads the current state).
-pub struct SubscriptionCompiler<'a> {
-    mailbox_cache: &'a dyn MailboxCache,
-    indexer_relays: &'a [RelayUrl],
-}
-
-impl<'a> SubscriptionCompiler<'a> {
-    /// Construct a compiler bound to a mailbox cache and indexer set.
-    pub fn new(mailbox_cache: &'a dyn MailboxCache, indexer_relays: &'a [RelayUrl]) -> Self {
-        Self { mailbox_cache, indexer_relays }
-    }
-
-    /// Compile a set of logical interests into a `CompiledPlan`.
-    ///
-    /// ## Stages
-    /// 1. Resolve each interest's authors to relay URLs via the mailbox cache.
-    ///    Authors are **partitioned** per relay — only the authors that declared
-    ///    a relay appear in that relay's sub-shape.
-    /// 2. Fall back missing authors to the indexer set.
-    /// 3. Group relay entries by relay URL; merge compatible shapes per Rules 1–8.
-    /// 4. Compute `plan_id` via a stable deterministic hash.
-    ///
-    /// # EventStore coverage (phase 1 placeholder)
-    /// The compiler does not yet consult `EventStore::coverage()` for cache-aware
-    /// planning. The call-point is reserved here for the phase 2 / M3 wiring:
-    ///
-    /// ```text
-    /// // TODO(phase2): let coverage = event_store.coverage(&watermark_key)?;
-    /// // Use coverage to skip REQs whose time-range is fully cached locally.
-    /// ```
-    pub fn compile(
-        &self,
-        interests: &[LogicalInterest],
-    ) -> Result<CompiledPlan, PlannerError> {
-        // ── Stages 1 + 2: author-partitioned relay entry collection ──────────
-        // relay_url → Vec<RelayEntry>
-        let mut relay_entries: BTreeMap<RelayUrl, Vec<RelayEntry>> = BTreeMap::new();
-
-        for interest in interests {
-            self.partition_interest(interest, &mut relay_entries);
-        }
-
-        // ── Stage 3: Per-relay shape merge ──────────────────────────────────
-        let mut per_relay: BTreeMap<RelayUrl, RelayPlan> = BTreeMap::new();
-
-        for (relay_url, entries) in relay_entries {
-            let mut role_tags: BTreeSet<RoutingSource> = BTreeSet::new();
-
-            // Convert RelayEntry → (shape, lifecycle, source, id)
-            let mut resolved: Vec<(InterestShape, InterestLifecycle, RoutingSource, InterestId)> =
-                entries
-                    .into_iter()
-                    .map(|entry| {
-                        let source = entry.source.clone();
-                        role_tags.insert(source);
-                        entry.into_shape()
-                    })
-                    .collect();
-
-            // Greedy pairwise merge
-            let mut sub_shapes: Vec<(InterestShape, InterestLifecycle, Vec<InterestId>)> =
-                Vec::new();
-
-            for (shape, lifecycle, _source, interest_id) in resolved.drain(..) {
-                let mut merged = false;
-                for (existing_shape, existing_lifecycle, existing_ids) in sub_shapes.iter_mut() {
-                    if let MergeOutcome::Merged(new_shape) =
-                        merge(&existing_shape.clone(), &shape, existing_lifecycle, &lifecycle)
-                    {
-                        *existing_shape = new_shape;
-                        existing_ids.push(interest_id.clone());
-                        merged = true;
-                        break;
-                    }
-                }
-                if !merged {
-                    sub_shapes.push((shape, lifecycle, vec![interest_id]));
-                }
-            }
-
-            let relay_sub_shapes: Vec<SubShape> = sub_shapes
-                .into_iter()
-                .map(|(shape, _lifecycle, ids)| {
-                    let hash = simple_shape_hash(&shape);
-                    SubShape {
-                        shape,
-                        originating_interests: ids,
-                        canonical_filter_hash: hash,
-                    }
-                })
-                .collect();
-
-            per_relay.insert(
-                relay_url.clone(),
-                RelayPlan {
-                    relay_url,
-                    role_tags,
-                    sub_shapes: relay_sub_shapes,
-                },
-            );
-        }
-
-        // ── Stage 4: Plan-id binding ──────────────────────────────────────────
-        let plan_id = compute_plan_id(interests, self.mailbox_cache, MERGE_LATTICE_VERSION);
-
-        Ok(CompiledPlan { plan_id, per_relay })
-    }
-
-    /// Stage 1 + 2: partition one logical interest into per-relay entries.
-    ///
-    /// Each entry carries only the AUTHORS that declared the specific relay,
-    /// preserving the per-relay author-subset semantics required by the audit
-    /// test (Assertion 2) and the design spec §3.3.
-    fn partition_interest(
-        &self,
-        interest: &LogicalInterest,
-        relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
-    ) {
-        // Base shape: everything except authors and addresses (will be filled per relay).
-        let base_shape = InterestShape {
-            authors: BTreeSet::new(),
-            kinds: interest.shape.kinds.clone(),
-            tags: interest.shape.tags.clone(),
-            since: interest.shape.since,
-            until: interest.shape.until,
-            limit: interest.shape.limit,
-            event_ids: interest.shape.event_ids.clone(),
-            addresses: BTreeSet::new(),
-        };
-
-        // Case A: interest has explicit authors — partition them.
-        // Also routes any address-pointer coordinates in the same interest, since
-        // both authors and addresses may target the same relay (or different ones).
-        if !interest.shape.authors.is_empty() {
-            // relay_url → (author_set, addr_set, source)
-            let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)> =
-                BTreeMap::new();
-
-            for author in &interest.shape.authors {
-                match self.mailbox_cache.get(author) {
-                    Some(snapshot) => {
-                        for relay in snapshot.outbox_relays() {
-                            let entry =
-                                per_relay.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
-                                });
-                            entry.0.insert(author.clone());
-                        }
-                    }
-                    None => {
-                        // Indexer fallback (Stage 2)
-                        for relay in self.indexer_relays {
-                            let entry =
-                                per_relay.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Indexer)
-                                });
-                            entry.0.insert(author.clone());
-                        }
-                    }
-                }
-            }
-
-            // Route address-pointer coordinates (may target the same relays or different ones).
-            for coord in &interest.shape.addresses {
-                match self.mailbox_cache.get(&coord.pubkey) {
-                    Some(snapshot) => {
-                        for relay in snapshot.outbox_relays() {
-                            let entry =
-                                per_relay.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
-                                });
-                            entry.1.insert(coord.clone());
-                        }
-                    }
-                    None => {
-                        for relay in self.indexer_relays {
-                            let entry =
-                                per_relay.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), BTreeSet::new(), RoutingSource::Indexer)
-                                });
-                            entry.1.insert(coord.clone());
-                        }
-                    }
-                }
-            }
-
-            for (relay_url, (authors, addrs, source)) in per_relay {
-                relay_entries.entry(relay_url).or_default().push(RelayEntry {
-                    base_shape: base_shape.clone(),
-                    authors_for_relay: authors,
-                    addresses_for_relay: addrs,
-                    lifecycle: interest.lifecycle.clone(),
-                    source,
-                    interest_id: interest.id.clone(),
-                });
-            }
-            return;
-        }
-
-        // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
-        if !interest.shape.addresses.is_empty() {
-            // relay_url → (coord_set, source)
-            let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, RoutingSource)> =
-                BTreeMap::new();
-
-            for coord in &interest.shape.addresses {
-                match self.mailbox_cache.get(&coord.pubkey) {
-                    Some(snapshot) => {
-                        for relay in snapshot.outbox_relays() {
-                            let entry =
-                                per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), RoutingSource::Nip65)
-                                });
-                            entry.0.insert(coord.clone());
-                        }
-                    }
-                    None => {
-                        for relay in self.indexer_relays {
-                            let entry =
-                                per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
-                                    (BTreeSet::new(), RoutingSource::Indexer)
-                                });
-                            entry.0.insert(coord.clone());
-                        }
-                    }
-                }
-            }
-
-            for (relay_url, (addrs, source)) in per_relay_addrs {
-                relay_entries.entry(relay_url).or_default().push(RelayEntry {
-                    base_shape: base_shape.clone(),
-                    authors_for_relay: BTreeSet::new(),
-                    addresses_for_relay: addrs,
-                    lifecycle: interest.lifecycle.clone(),
-                    source,
-                    interest_id: interest.id.clone(),
-                });
-            }
-            return;
-        }
-
-        // Case C: no author, no addresses — route to indexer set (e.g. hashtag firehose).
-        for relay in self.indexer_relays {
-            relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
-                base_shape: base_shape.clone(),
-                authors_for_relay: BTreeSet::new(),
-                addresses_for_relay: BTreeSet::new(),
-                lifecycle: interest.lifecycle.clone(),
-                source: RoutingSource::Indexer,
-                interest_id: interest.id.clone(),
-            });
-        }
-    }
-}
-
-// ─── Plan-id hashing ─────────────────────────────────────────────────────────
-
-/// Compute a stable, deterministic plan-id string.
-///
-/// The hash covers: sorted interest ids + shapes + scopes + lifecycles,
-/// the mailbox snapshot sorted by pubkey, and the lattice version.
-///
-/// Phase 1: uses a simple FNV-1a accumulation. Phase 2 will upgrade to
-/// blake3 when that crate is in the workspace.
-fn compute_plan_id(
-    interests: &[LogicalInterest],
-    cache: &dyn MailboxCache,
-    lattice_version: u8,
-) -> String {
-    struct FnvHasher(u64);
-    impl FnvHasher {
-        fn new() -> Self {
-            Self(0xcbf29ce484222325)
-        }
-        fn feed_bytes(&mut self, bytes: &[u8]) {
-            for &b in bytes {
-                self.0 ^= u64::from(b);
-                self.0 = self.0.wrapping_mul(0x100000001b3);
-            }
-        }
-        fn finish(self) -> u64 {
-            self.0
-        }
-    }
-
-    let mut h = FnvHasher::new();
-
-    // Sorted interest contributions
-    let mut sorted_interests: Vec<&LogicalInterest> = interests.iter().collect();
-    sorted_interests.sort_by_key(|i| &i.id);
-    for interest in sorted_interests {
-        // Feed id
-        h.feed_bytes(&interest.id.0.to_le_bytes());
-        // Feed shape via serde_json (deterministic because BTreeSet/BTreeMap)
-        if let Ok(shape_json) = serde_json::to_vec(&interest.shape) {
-            h.feed_bytes(&shape_json);
-        }
-        // Feed lifecycle tag byte
-        let lifecycle_tag: u8 = match &interest.lifecycle {
-            InterestLifecycle::Tailing => 0,
-            InterestLifecycle::OneShot => 1,
-            InterestLifecycle::BoundedTime { until_ms } => {
-                h.feed_bytes(&until_ms.to_le_bytes());
-                2
-            }
-        };
-        h.feed_bytes(&[lifecycle_tag]);
-    }
-
-    // Sorted mailbox snapshot
-    let mut snapshot = cache.snapshot_all();
-    snapshot.sort_by(|(a, _), (b, _)| a.cmp(b));
-    for (pk, mb) in snapshot {
-        h.feed_bytes(pk.as_bytes());
-        for r in &mb.write_relays {
-            h.feed_bytes(r.as_bytes());
-        }
-        for r in &mb.read_relays {
-            h.feed_bytes(r.as_bytes());
-        }
-        for r in &mb.both_relays {
-            h.feed_bytes(r.as_bytes());
-        }
-    }
-
-    // Lattice version
-    h.feed_bytes(&[lattice_version]);
-
-    format!("{:016x}", h.finish())
-}
-
-// ─── Canonical filter hash ────────────────────────────────────────────────────
-
-fn simple_shape_hash(shape: &InterestShape) -> String {
-    use std::collections::hash_map::DefaultHasher;
-    use std::hash::{Hash, Hasher};
-
-    let mut h = DefaultHasher::new();
-    if let Ok(json) = serde_json::to_string(shape) {
-        json.hash(&mut h);
-    }
-    format!("{:08x}", h.finish() & 0xffff_ffff)
-}
diff --git a/crates/nmp-core/src/planner/compiler/mailbox.rs b/crates/nmp-core/src/planner/compiler/mailbox.rs
new file mode 100644
index 0000000..a29743c
--- /dev/null
+++ b/crates/nmp-core/src/planner/compiler/mailbox.rs
@@ -0,0 +1,105 @@
+//! `MailboxCache` trait, `MailboxSnapshot`, and phase-1 implementations.
+//!
+//! The trait is the seam between the compiler and the `nmp-nip65` crate.
+//! Phase 1: `EmptyMailboxCache` + `InMemoryMailboxCache` stubs.
+//! Phase 2: replaced by `nmp-nip65::InMemoryMailboxCache`.
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3.1
+//! Doctrine: D3 (outbox routing automatic).
+
+use std::collections::HashMap;
+use crate::planner::interest::{Pubkey, RelayUrl};
+
+// ─── MailboxSnapshot ─────────────────────────────────────────────────────────
+
+/// Minimal mailbox snapshot used by the compiler.
+///
+/// Phase 1: only `write_relays` and `both_relays` are consumed (Outbox
+/// direction). Inbox direction (read_relays) is used for `#p` interests.
+///
+/// Full trait lives in `nmp-nip65::cache::MailboxCache` (later slice).
+#[derive(Clone, Debug, Default)]
+pub struct MailboxSnapshot {
+    pub write_relays: Vec<RelayUrl>,
+    pub read_relays: Vec<RelayUrl>,
+    pub both_relays: Vec<RelayUrl>,
+}
+
+impl MailboxSnapshot {
+    /// All relays relevant for Outbox direction (write + both).
+    pub fn outbox_relays(&self) -> impl Iterator<Item = &RelayUrl> {
+        self.write_relays.iter().chain(self.both_relays.iter())
+    }
+}
+
+// ─── MailboxCache trait ───────────────────────────────────────────────────────
+
+/// Minimum surface the compiler needs for mailbox lookups.
+/// Phase 1 implementation: `EmptyMailboxCache` always returns `None`.
+/// Phase 2 implementation: `nmp-nip65::InMemoryMailboxCache`.
+pub trait MailboxCache: Send + Sync {
+    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
+    /// Snapshot of all known entries for plan-id hashing.
+    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
+    /// Monotonic generation counter — advances on every accepted `put`.
+    fn generation(&self) -> u64;
+    /// Request a background probe for a pubkey whose mailbox is unknown.
+    ///
+    /// Phase 1: no-op. Phase 2: the actor wires this to an `IndexerProbe`
+    /// action that fetches the author's kind:10002 from the indexer set,
+    /// then calls `put()` on cache arrival, triggering a recompile.
+    ///
+    /// Design: `docs/design/subscription-compilation/compiler.md` §3.2
+    fn request_probe(&self, _pubkey: &Pubkey) {
+        // Default: no-op. Implementations that own an action channel override this.
+    }
+}
+
+// ─── EmptyMailboxCache ───────────────────────────────────────────────────────
+
+/// Phase 1 stub: no mailbox data. All authors fall back to the indexer set.
+pub struct EmptyMailboxCache;
+
+impl MailboxCache for EmptyMailboxCache {
+    fn get(&self, _pubkey: &Pubkey) -> Option<MailboxSnapshot> {
+        None
+    }
+    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
+        Vec::new()
+    }
+    fn generation(&self) -> u64 {
+        0
+    }
+}
+
+// ─── InMemoryMailboxCache ────────────────────────────────────────────────────
+
+/// Simple in-memory mailbox cache for tests and the planner harness.
+#[derive(Default)]
+pub struct InMemoryMailboxCache {
+    data: HashMap<Pubkey, MailboxSnapshot>,
+    generation: u64,
+}
+
+impl InMemoryMailboxCache {
+    pub fn new() -> Self {
+        Self::default()
+    }
+
+    pub fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot) {
+        self.data.insert(pubkey, snapshot);
+        self.generation = self.generation.saturating_add(1);
+    }
+}
+
+impl MailboxCache for InMemoryMailboxCache {
+    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
+        self.data.get(pubkey).cloned()
+    }
+    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
+        self.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
+    }
+    fn generation(&self) -> u64 {
+        self.generation
+    }
+}
diff --git a/crates/nmp-core/src/planner/compiler/mod.rs b/crates/nmp-core/src/planner/compiler/mod.rs
new file mode 100644
index 0000000..24e5a89
--- /dev/null
+++ b/crates/nmp-core/src/planner/compiler/mod.rs
@@ -0,0 +1,191 @@
+//! The subscription compiler: 4-stage pipeline from `Vec<LogicalInterest>`
+//! to `CompiledPlan`.
+//!
+//! ## Pipeline stages
+//!
+//! 1. **Resolve authors → mailboxes** — consult `MailboxCache` (phase 1 stub:
+//!    `EmptyMailboxCache`; real impl in `nmp-nip65`).
+//! 2. **Indexer fallback** — authors with no known mailbox route to the
+//!    configured indexer set.
+//! 3. **Per-relay shape merge** — group by relay URL; merge compatible shapes
+//!    with `lattice::merge()` (Rules 1–8). Author sets are partitioned per
+//!    relay — only authors that declared a relay appear in its sub-shape.
+//! 4. **Plan-id binding** — deterministic hash → stable `plan_id`.
+//!
+//! ## Module structure
+//!
+//! - `mailbox`   — `MailboxCache` trait + `MailboxSnapshot` + phase-1 impls.
+//! - `plan_id`   — `CompileContext` + `compute_plan_id` (FNV-1a hash).
+//! - `partition` — `RelayEntry` + `partition_interest` (Stage 1+2).
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3
+//! Doctrine: D3 (outbox routing automatic), D6 (errors never cross FFI),
+//!           D8 (zero per-event allocs after warmup).
+
+mod mailbox;
+mod partition;
+mod plan_id;
+
+pub use mailbox::{EmptyMailboxCache, InMemoryMailboxCache, MailboxCache, MailboxSnapshot};
+pub use plan_id::CompileContext;
+
+use std::collections::{BTreeMap, BTreeSet};
+
+use crate::planner::{
+    interest::{InterestId, InterestLifecycle, InterestShape, LogicalInterest, RelayUrl},
+    lattice::{merge, MergeOutcome},
+    plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape},
+};
+use partition::{partition_interest, RelayEntry};
+use plan_id::compute_plan_id;
+
+/// Version of the merge lattice — bump when Rule semantics change.
+const MERGE_LATTICE_VERSION: u8 = 1;
+
+// ─── SubscriptionCompiler ────────────────────────────────────────────────────
+
+/// The subscription compiler.
+///
+/// Holds a reference to the mailbox cache and indexer relay set. Both may be
+/// updated between compilations (the compiler always reads the current state).
+///
+/// ## Direction table (§3.1 / §3.2)
+///
+/// | Interest shape          | Direction | Relay source                                     |
+/// |-------------------------|-----------|--------------------------------------------------|
+/// | Has `authors`           | Outbox    | author's write relays via NIP-65 (or indexer)    |
+/// | Has `#p` tag values     | Inbox     | tagged pubkey's read relays (post-v1 DMs/notifs) |
+/// | Has `addresses`         | Outbox    | coord.pubkey's write relays                      |
+/// | No author/addr/p        | Read      | active-account read relays (hashtag firehose)    |
+pub struct SubscriptionCompiler<'a> {
+    mailbox_cache: &'a dyn MailboxCache,
+    indexer_relays: &'a [RelayUrl],
+    /// Active account read relays — for no-author/no-address interests.
+    /// Phase 1: empty → falls through to indexer set.
+    /// Phase 2: populated from active account's kind:10002 read-relays.
+    active_account_read_relays: &'a [RelayUrl],
+}
+
+impl<'a> SubscriptionCompiler<'a> {
+    /// Construct a compiler bound to a mailbox cache and indexer set.
+    pub fn new(mailbox_cache: &'a dyn MailboxCache, indexer_relays: &'a [RelayUrl]) -> Self {
+        Self { mailbox_cache, indexer_relays, active_account_read_relays: &[] }
+    }
+
+    /// Construct with explicit active-account read relays.
+    ///
+    /// When `active_account_read_relays` is non-empty, no-author interests
+    /// (hashtag firehose, global search) route to those relays instead of the
+    /// indexer set, using `RoutingSource::UserConfigured(AccountRead)`.
+    pub fn with_active_account_read_relays(
+        mailbox_cache: &'a dyn MailboxCache,
+        indexer_relays: &'a [RelayUrl],
+        active_account_read_relays: &'a [RelayUrl],
+    ) -> Self {
+        Self { mailbox_cache, indexer_relays, active_account_read_relays }
+    }
+
+    /// Compile a set of logical interests into a `CompiledPlan`.
+    ///
+    /// Equivalent to `compile_with_context(interests, &CompileContext::default())`.
+    /// Use `compile_with_context` when tracking policy version counters.
+    ///
+    /// # EventStore coverage
+    /// Phase 2 / M3 wiring point (not yet consulted):
+    /// ```text
+    /// // Phase 2: let coverage = event_store.coverage(&watermark_key)?;
+    /// // Use coverage to skip REQs whose time-range is fully cached locally.
+    /// ```
+    pub fn compile(
+        &self,
+        interests: &[LogicalInterest],
+    ) -> Result<CompiledPlan, PlannerError> {
+        self.compile_with_context(interests, &CompileContext::default())
+    }
+
+    /// Compile with explicit versioning context for plan-id stability.
+    ///
+    /// Callers that track `indexer_set_version` / `user_config_version` should
+    /// use this form so plan-ids invalidate correctly on policy changes.
+    pub fn compile_with_context(
+        &self,
+        interests: &[LogicalInterest],
+        ctx: &CompileContext,
+    ) -> Result<CompiledPlan, PlannerError> {
+        // ── Stages 1 + 2: author-partitioned relay entry collection ──────────
+        let mut relay_entries: BTreeMap<RelayUrl, Vec<RelayEntry>> = BTreeMap::new();
+        for interest in interests {
+            partition_interest(
+                interest,
+                self.mailbox_cache,
+                self.indexer_relays,
+                self.active_account_read_relays,
+                &mut relay_entries,
+            );
+        }
+
+        // ── Stage 3: Per-relay shape merge ──────────────────────────────────
+        let mut per_relay: BTreeMap<RelayUrl, RelayPlan> = BTreeMap::new();
+        for (relay_url, entries) in relay_entries {
+            let mut role_tags: BTreeSet<RoutingSource> = BTreeSet::new();
+            let mut resolved: Vec<(InterestShape, InterestLifecycle, RoutingSource, InterestId)> =
+                entries
+                    .into_iter()
+                    .map(|entry| {
+                        let source = entry.source.clone();
+                        role_tags.insert(source);
+                        entry.into_shape()
+                    })
+                    .collect();
+
+            let mut sub_shapes: Vec<(InterestShape, InterestLifecycle, Vec<InterestId>)> =
+                Vec::new();
+            for (shape, lifecycle, _source, interest_id) in resolved.drain(..) {
+                let mut merged = false;
+                for (existing_shape, existing_lifecycle, existing_ids) in sub_shapes.iter_mut() {
+                    if let MergeOutcome::Merged(new_shape) =
+                        merge(&existing_shape.clone(), &shape, existing_lifecycle, &lifecycle)
+                    {
+                        *existing_shape = new_shape;
+                        existing_ids.push(interest_id.clone());
+                        merged = true;
+                        break;
+                    }
+                }
+                if !merged {
+                    sub_shapes.push((shape, lifecycle, vec![interest_id]));
+                }
+            }
+
+            let relay_sub_shapes: Vec<SubShape> = sub_shapes
+                .into_iter()
+                .map(|(shape, _lifecycle, ids)| {
+                    let hash = simple_shape_hash(&shape);
+                    SubShape { shape, originating_interests: ids, canonical_filter_hash: hash }
+                })
+                .collect();
+
+            per_relay.insert(
+                relay_url.clone(),
+                RelayPlan { relay_url, role_tags, sub_shapes: relay_sub_shapes },
+            );
+        }
+
+        // ── Stage 4: Plan-id binding ──────────────────────────────────────────
+        let plan_id = compute_plan_id(interests, self.mailbox_cache, ctx, MERGE_LATTICE_VERSION);
+        Ok(CompiledPlan { plan_id, per_relay })
+    }
+}
+
+// ─── Canonical filter hash ────────────────────────────────────────────────────
+
+fn simple_shape_hash(shape: &InterestShape) -> String {
+    use std::collections::hash_map::DefaultHasher;
+    use std::hash::{Hash, Hasher};
+
+    let mut h = DefaultHasher::new();
+    if let Ok(json) = serde_json::to_string(shape) {
+        json.hash(&mut h);
+    }
+    format!("{:08x}", h.finish() & 0xffff_ffff)
+}
diff --git a/crates/nmp-core/src/planner/compiler/partition.rs b/crates/nmp-core/src/planner/compiler/partition.rs
new file mode 100644
index 0000000..33f42e5
--- /dev/null
+++ b/crates/nmp-core/src/planner/compiler/partition.rs
@@ -0,0 +1,258 @@
+//! `RelayEntry` and `partition_interest`: Stage 1+2 of the compiler pipeline.
+//!
+//! Partitions a single `LogicalInterest` into per-relay entries, with each
+//! entry carrying only the authors that declared the relay (author-partitioning).
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
+//! Doctrine: D3 (outbox routing automatic).
+
+use std::collections::{BTreeMap, BTreeSet};
+
+use crate::planner::{
+    interest::{
+        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
+        RelayUrl,
+    },
+    plan::{RoutingSource, UserConfiguredCategory},
+};
+use super::mailbox::MailboxCache;
+
+// ─── RelayEntry ──────────────────────────────────────────────────────────────
+
+/// A relay-partitioned slice of one logical interest.
+///
+/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
+/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
+/// authors that declared this specific relay (not all N authors). This is the
+/// author-partitioning that lets the merge lattice produce per-relay author
+/// subsets.
+pub(super) struct RelayEntry {
+    /// The interest's non-author fields (kinds, tags, since, until, etc.).
+    /// `authors` is intentionally left empty here; we merge `authors_for_relay`
+    /// in at Stage 3 merge time.
+    pub base_shape: InterestShape,
+    /// The subset of authors from this interest that declared this relay.
+    pub authors_for_relay: BTreeSet<Pubkey>,
+    /// Address-pointer coordinates from this interest (if relevant for routing).
+    pub addresses_for_relay: BTreeSet<NaddrCoord>,
+    pub lifecycle: InterestLifecycle,
+    pub source: RoutingSource,
+    pub interest_id: InterestId,
+}
+
+impl RelayEntry {
+    /// Construct the final `InterestShape` for this relay slice.
+    pub fn into_shape(mut self) -> (InterestShape, InterestLifecycle, RoutingSource, InterestId) {
+        self.base_shape.authors = self.authors_for_relay;
+        self.base_shape.addresses = self.addresses_for_relay;
+        (self.base_shape, self.lifecycle, self.source, self.interest_id)
+    }
+}
+
+// ─── partition_interest ───────────────────────────────────────────────────────
+
+/// Stage 1 + 2: partition one logical interest into per-relay entries.
+///
+/// Each entry carries only the AUTHORS that declared the specific relay,
+/// preserving per-relay author-subset semantics (Assertion 2, §3.3).
+///
+/// ## Direction routing (§3.1 / §3.2)
+///
+/// - **Case A**: explicit `authors` → Outbox (write relays). Also routes
+///   any `addresses` on the same interest to the same relay map.
+/// - **Case B**: no authors, but `addresses` → Outbox for each coord.pubkey.
+/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
+///   (tagged pubkey's read relays). Structural ban enforced: never route
+///   private `#p` interests to non-inbox relays.
+///   Phase 1 stub: falls back to indexer; real inbox resolution in phase 2.
+/// - **Case D (no-author)**: no authors, addresses, or #p → active-account
+///   read relays (hashtag firehose, global search). Falls to indexer if empty.
+pub(super) fn partition_interest(
+    interest: &LogicalInterest,
+    mailbox_cache: &dyn MailboxCache,
+    indexer_relays: &[RelayUrl],
+    active_account_read_relays: &[RelayUrl],
+    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
+) {
+    let base_shape = InterestShape {
+        authors: BTreeSet::new(),
+        kinds: interest.shape.kinds.clone(),
+        tags: interest.shape.tags.clone(),
+        since: interest.shape.since,
+        until: interest.shape.until,
+        limit: interest.shape.limit,
+        event_ids: interest.shape.event_ids.clone(),
+        addresses: BTreeSet::new(),
+    };
+
+    // Case A: explicit authors → Outbox (write relays).
+    if !interest.shape.authors.is_empty() {
+        let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)> =
+            BTreeMap::new();
+
+        for author in &interest.shape.authors {
+            match mailbox_cache.get(author) {
+                Some(snapshot) => {
+                    for relay in snapshot.outbox_relays() {
+                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
+                        });
+                        entry.0.insert(author.clone());
+                    }
+                }
+                None => {
+                    for relay in indexer_relays {
+                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(), BTreeSet::new(),
+                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
+                        });
+                        entry.0.insert(author.clone());
+                    }
+                }
+            }
+        }
+
+        for coord in &interest.shape.addresses {
+            match mailbox_cache.get(&coord.pubkey) {
+                Some(snapshot) => {
+                    for relay in snapshot.outbox_relays() {
+                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
+                        });
+                        entry.1.insert(coord.clone());
+                    }
+                }
+                None => {
+                    for relay in indexer_relays {
+                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(), BTreeSet::new(),
+                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
+                        });
+                        entry.1.insert(coord.clone());
+                    }
+                }
+            }
+        }
+
+        for (relay_url, (authors, addrs, source)) in per_relay {
+            relay_entries.entry(relay_url).or_default().push(RelayEntry {
+                base_shape: base_shape.clone(),
+                authors_for_relay: authors,
+                addresses_for_relay: addrs,
+                lifecycle: interest.lifecycle.clone(),
+                source,
+                interest_id: interest.id.clone(),
+            });
+        }
+        return;
+    }
+
+    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
+    if !interest.shape.addresses.is_empty() {
+        let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, RoutingSource)> =
+            BTreeMap::new();
+
+        for coord in &interest.shape.addresses {
+            match mailbox_cache.get(&coord.pubkey) {
+                Some(snapshot) => {
+                    for relay in snapshot.outbox_relays() {
+                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(), RoutingSource::Nip65)
+                        });
+                        entry.0.insert(coord.clone());
+                    }
+                }
+                None => {
+                    for relay in indexer_relays {
+                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
+                            (BTreeSet::new(),
+                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
+                        });
+                        entry.0.insert(coord.clone());
+                    }
+                }
+            }
+        }
+
+        for (relay_url, (addrs, source)) in per_relay_addrs {
+            relay_entries.entry(relay_url).or_default().push(RelayEntry {
+                base_shape: base_shape.clone(),
+                authors_for_relay: BTreeSet::new(),
+                addresses_for_relay: addrs,
+                lifecycle: interest.lifecycle.clone(),
+                source,
+                interest_id: interest.id.clone(),
+            });
+        }
+        return;
+    }
+
+    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
+    //
+    // #p interests (DMs, notifications) MUST route to the tagged pubkey's READ
+    // relays (Inbox direction). Routing them to write relays violates the
+    // structural ban on private routes to non-inbox relays (§3.2).
+    //
+    // Phase 1 stub: read_relays not yet populated from kind:10002 → fall back
+    // to indexer. The code path is correct; only the mailbox data is missing.
+    let p_tag_values: BTreeSet<Pubkey> = interest
+        .shape
+        .tags
+        .get("p")
+        .cloned()
+        .unwrap_or_default();
+
+    if !p_tag_values.is_empty() {
+        for tagged_pk in &p_tag_values {
+            match mailbox_cache.get(tagged_pk) {
+                Some(snapshot) if !snapshot.read_relays.is_empty() => {
+                    for relay in &snapshot.read_relays {
+                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
+                            base_shape: base_shape.clone(),
+                            authors_for_relay: BTreeSet::new(),
+                            addresses_for_relay: BTreeSet::new(),
+                            lifecycle: interest.lifecycle.clone(),
+                            source: RoutingSource::Nip65,
+                            interest_id: interest.id.clone(),
+                        });
+                    }
+                }
+                _ => {
+                    mailbox_cache.request_probe(tagged_pk);
+                    for relay in indexer_relays {
+                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
+                            base_shape: base_shape.clone(),
+                            authors_for_relay: BTreeSet::new(),
+                            addresses_for_relay: BTreeSet::new(),
+                            lifecycle: interest.lifecycle.clone(),
+                            source: RoutingSource::UserConfigured(
+                                UserConfiguredCategory::Indexer,
+                            ),
+                            interest_id: interest.id.clone(),
+                        });
+                    }
+                }
+            }
+        }
+        return;
+    }
+
+    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
+    let (fallback_relays, fallback_source) = if !active_account_read_relays.is_empty() {
+        (active_account_read_relays,
+         RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead))
+    } else {
+        (indexer_relays,
+         RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
+    };
+    for relay in fallback_relays {
+        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
+            base_shape: base_shape.clone(),
+            authors_for_relay: BTreeSet::new(),
+            addresses_for_relay: BTreeSet::new(),
+            lifecycle: interest.lifecycle.clone(),
+            source: fallback_source.clone(),
+            interest_id: interest.id.clone(),
+        });
+    }
+}
diff --git a/crates/nmp-core/src/planner/compiler/plan_id.rs b/crates/nmp-core/src/planner/compiler/plan_id.rs
new file mode 100644
index 0000000..4c59195
--- /dev/null
+++ b/crates/nmp-core/src/planner/compiler/plan_id.rs
@@ -0,0 +1,159 @@
+//! Plan-id hashing: `CompileContext` and `compute_plan_id`.
+//!
+//! The plan-id is a content-addressed string that uniquely identifies a
+//! compiled plan. It covers only the inputs that actually affect routing:
+//! referenced pubkeys (not the full mailbox cache), interest shapes, scopes,
+//! and version counters.
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3.4
+//! Doctrine: D8 (plan-id stability avoids redundant recompilation).
+
+use std::collections::BTreeSet;
+use crate::planner::interest::{InterestLifecycle, InterestScope, LogicalInterest, Pubkey};
+use super::mailbox::MailboxCache;
+
+// ─── CompileContext ───────────────────────────────────────────────────────────
+
+/// Versioning inputs for plan-id binding (§3.4).
+///
+/// Both counters advance whenever the corresponding policy changes:
+/// - `indexer_set_version` — bumped when the kernel's indexer relay set changes.
+/// - `user_config_version` — bumped when user-configured relay settings change.
+///
+/// Including these in the plan-id hash ensures that plan-ids invalidate when
+/// policy changes even if the interest set itself is unchanged.
+///
+/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
+#[derive(Clone, Debug, Default)]
+pub struct CompileContext {
+    /// Monotonic counter advancing on every accepted change to the indexer set.
+    pub indexer_set_version: u64,
+    /// Monotonic counter advancing on every accepted change to user-configured relays.
+    pub user_config_version: u64,
+}
+
+// ─── FNV-1a hasher ───────────────────────────────────────────────────────────
+
+/// FNV-1a hasher (64-bit).
+///
+/// Phase 1 implementation. Phase 2 will upgrade to blake3 when that crate
+/// joins the workspace.
+struct FnvHasher(u64);
+
+impl FnvHasher {
+    fn new() -> Self {
+        Self(0xcbf29ce484222325)
+    }
+    fn feed_bytes(&mut self, bytes: &[u8]) {
+        for &b in bytes {
+            self.0 ^= u64::from(b);
+            self.0 = self.0.wrapping_mul(0x100000001b3);
+        }
+    }
+    fn feed_u64(&mut self, v: u64) {
+        self.feed_bytes(&v.to_le_bytes());
+    }
+    fn finish(self) -> u64 {
+        self.0
+    }
+}
+
+// ─── Referenced pubkeys ───────────────────────────────────────────────────────
+
+/// Collect all pubkeys that are referenced by the interest set.
+///
+/// Per §3.4: only the mailbox entries for **referenced** pubkeys participate
+/// in the plan-id hash. An unrelated kind:10002 arrival (for a pubkey not in
+/// any interest's author set, #p tags, or address pubkeys) MUST NOT change
+/// the plan-id.
+///
+/// Referenced pubkeys = `interest.shape.authors ∪ addresses[*].pubkey ∪ tags["p"][*]`
+pub(super) fn referenced_pubkeys(interests: &[LogicalInterest]) -> BTreeSet<Pubkey> {
+    let mut pks = BTreeSet::new();
+    for interest in interests {
+        pks.extend(interest.shape.authors.iter().cloned());
+        for coord in &interest.shape.addresses {
+            pks.insert(coord.pubkey.clone());
+        }
+        if let Some(p_values) = interest.shape.tags.get("p") {
+            pks.extend(p_values.iter().cloned());
+        }
+    }
+    pks
+}
+
+// ─── compute_plan_id ─────────────────────────────────────────────────────────
+
+/// Compute a stable, deterministic plan-id string.
+///
+/// Hash inputs (all sorted for determinism):
+/// 1. Sorted interests: id + shape (JSON) + scope + lifecycle.
+/// 2. Mailbox snapshot for ONLY referenced pubkeys (§3.4 stability rule).
+///    Relay vectors within each snapshot are sorted before hashing.
+/// 3. Compile context: `indexer_set_version` + `user_config_version`.
+/// 4. Merge lattice version.
+///
+/// An unrelated kind:10002 arrival (for a pubkey not in any interest's author
+/// set / #p tags / address pubkeys) MUST NOT change the plan-id.
+pub(super) fn compute_plan_id(
+    interests: &[LogicalInterest],
+    cache: &dyn MailboxCache,
+    ctx: &CompileContext,
+    lattice_version: u8,
+) -> String {
+    let mut h = FnvHasher::new();
+
+    // ── 1. Sorted interest contributions ─────────────────────────────────────
+    let mut sorted_interests: Vec<&LogicalInterest> = interests.iter().collect();
+    sorted_interests.sort_by_key(|i| &i.id);
+    for interest in sorted_interests {
+        h.feed_u64(interest.id.0);
+        if let Ok(shape_json) = serde_json::to_vec(&interest.shape) {
+            h.feed_bytes(&shape_json);
+        }
+        let scope_tag: u8 = match &interest.scope {
+            InterestScope::ActiveAccount => 0,
+            InterestScope::Account(acct) => {
+                h.feed_bytes(acct.as_bytes());
+                1
+            }
+            InterestScope::Global => 2,
+        };
+        h.feed_bytes(&[scope_tag]);
+        let lifecycle_tag: u8 = match &interest.lifecycle {
+            InterestLifecycle::Tailing => 0,
+            InterestLifecycle::OneShot => 1,
+            InterestLifecycle::BoundedTime { until_ms } => {
+                h.feed_u64(*until_ms);
+                2
+            }
+        };
+        h.feed_bytes(&[lifecycle_tag]);
+    }
+
+    // ── 2. Mailbox snapshot — referenced pubkeys only ─────────────────────────
+    let ref_pks = referenced_pubkeys(interests);
+    for pk in &ref_pks {
+        if let Some(mb) = cache.get(pk) {
+            h.feed_bytes(pk.as_bytes());
+            let mut write_sorted = mb.write_relays.clone();
+            write_sorted.sort();
+            for r in &write_sorted { h.feed_bytes(r.as_bytes()); }
+            let mut read_sorted = mb.read_relays.clone();
+            read_sorted.sort();
+            for r in &read_sorted { h.feed_bytes(r.as_bytes()); }
+            let mut both_sorted = mb.both_relays.clone();
+            both_sorted.sort();
+            for r in &both_sorted { h.feed_bytes(r.as_bytes()); }
+        }
+    }
+
+    // ── 3. Compile context ────────────────────────────────────────────────────
+    h.feed_u64(ctx.indexer_set_version);
+    h.feed_u64(ctx.user_config_version);
+
+    // ── 4. Lattice version ────────────────────────────────────────────────────
+    h.feed_bytes(&[lattice_version]);
+
+    format!("{:016x}", h.finish())
+}
diff --git a/crates/nmp-core/src/planner/interest.rs b/crates/nmp-core/src/planner/interest.rs
index abf8dfb..fbeac3f 100644
--- a/crates/nmp-core/src/planner/interest.rs
+++ b/crates/nmp-core/src/planner/interest.rs
@@ -57,8 +57,8 @@ pub struct NaddrCoord {
     pub d_tag: String,
 }
 
-// TODO(nmp-nip19): add NaddrCoord::from_naddr_bech32 / to_naddr_bech32 helpers
-// once the nmp-nip19 crate (bech32 codec) lands in the workspace. Both helpers
+// Phase 2 (nmp-nip19): NaddrCoord::from_naddr_bech32 / to_naddr_bech32 helpers
+// land when the nmp-nip19 bech32 codec crate joins the workspace. Both helpers
 // are needed for the ThreadViewModule and MetaTimelineViewModule address-pointer
 // loaders to accept user-facing naddr strings from the Swift/Kotlin FFI surface.
 
diff --git a/crates/nmp-core/src/planner/lattice.rs b/crates/nmp-core/src/planner/lattice/mod.rs
similarity index 71%
rename from crates/nmp-core/src/planner/lattice.rs
rename to crates/nmp-core/src/planner/lattice/mod.rs
index efc13bd..82e6633 100644
--- a/crates/nmp-core/src/planner/lattice.rs
+++ b/crates/nmp-core/src/planner/lattice/mod.rs
@@ -2,6 +2,10 @@
 //! design. Only shapes that pass all eight rules are merged; otherwise the
 //! caller emits two distinct REQs.
 //!
+//! ## Module structure
+//!
+//! - `rules` — the 8 individual rule functions (pub(super)).
+//!
 //! Design: `docs/design/subscription-compilation/compiler.md` §3.3
 //! Doctrine: D8 (zero per-event allocs on the hot path after warmup).
 //!
@@ -15,7 +19,13 @@
 //! 7. `event_ids` — union, capped.
 //! 8. `addresses` — union, capped; requires other fields mergeable per 1–7.
 
-use super::interest::{InterestLifecycle, InterestShape};
+mod rules;
+
+use crate::planner::interest::{InterestLifecycle, InterestShape};
+use rules::{
+    rule1_kinds, rule2_tags, rule3_since, rule4_until, rule5_limit, rule6_lifecycle,
+    rule7_event_ids, rule8_addresses,
+};
 
 /// Per-relay cap for merged value sets (tags, ids, addresses).
 /// This mirrors the relay default of 1000 per filter.
@@ -36,7 +46,12 @@ pub enum MergeOutcome {
 /// Neither `a` nor `b` is modified on refusal.
 ///
 /// Design: §3.3 Rules 1–8
-pub fn merge(a: &InterestShape, b: &InterestShape, lifecycle_a: &InterestLifecycle, lifecycle_b: &InterestLifecycle) -> MergeOutcome {
+pub fn merge(
+    a: &InterestShape,
+    b: &InterestShape,
+    lifecycle_a: &InterestLifecycle,
+    lifecycle_b: &InterestLifecycle,
+) -> MergeOutcome {
     // Rule 6 first — cheapest check, prune early.
     if !rule6_lifecycle(lifecycle_a, lifecycle_b) {
         return MergeOutcome::Refused;
@@ -95,131 +110,6 @@ pub fn merge(a: &InterestShape, b: &InterestShape, lifecycle_a: &InterestLifecyc
     })
 }
 
-// ─── Individual rules ─────────────────────────────────────────────────────────
-
-/// Rule 1 — `kinds` merge.
-///
-/// Mergeable iff `a.kinds == b.kinds` OR one is empty (wildcard absorbs).
-/// Returns the merged kinds set, or `None` to refuse.
-fn rule1_kinds(
-    a: &InterestShape,
-    b: &InterestShape,
-) -> Option<std::collections::BTreeSet<u32>> {
-    if a.kinds == b.kinds {
-        Some(a.kinds.clone())
-    } else if a.kinds.is_empty() {
-        // a is wildcard — wildcard absorbs
-        Some(b.kinds.clone())
-    } else if b.kinds.is_empty() {
-        Some(a.kinds.clone())
-    } else {
-        // Both non-empty but different — refuse (merging would widen kinds)
-        None
-    }
-}
-
-/// Rule 2 — `tags` merge.
-///
-/// Mergeable iff both shapes have the same tag key dimensions, AND the union
-/// of values per dimension stays under `limit`.
-fn rule2_tags(
-    a: &InterestShape,
-    b: &InterestShape,
-    limit: usize,
-) -> Option<std::collections::BTreeMap<super::interest::TagKey, std::collections::BTreeSet<String>>> {
-    // Keys must be identical (same dimensions)
-    if a.tags.keys().ne(b.tags.keys()) {
-        return None;
-    }
-
-    let mut merged = std::collections::BTreeMap::new();
-    for (key, av) in &a.tags {
-        let bv = b.tags.get(key)?; // key must exist in b (already checked above)
-        let union: std::collections::BTreeSet<String> = av.union(bv).cloned().collect();
-        if union.len() > limit {
-            return None;
-        }
-        merged.insert(key.clone(), union);
-    }
-    Some(merged)
-}
-
-/// Rule 3 — `since` merge.
-///
-/// Returns `min(a.since, b.since)` iff both are `Some` or both are `None`.
-/// Mixed (one bounded, one unbounded) returns `None` (refuse).
-fn rule3_since(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
-    match (a.since, b.since) {
-        (None, None) => Some(None),
-        (Some(sa), Some(sb)) => Some(Some(sa.min(sb))),
-        _ => None, // Mixed — refuse
-    }
-}
-
-/// Rule 4 — `until` merge.
-///
-/// Returns `max(a.until, b.until)` iff both are `Some` or both are `None`.
-/// Mixed returns `None` (refuse).
-fn rule4_until(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
-    match (a.until, b.until) {
-        (None, None) => Some(None),
-        (Some(ua), Some(ub)) => Some(Some(ua.max(ub))),
-        _ => None, // Mixed — refuse
-    }
-}
-
-/// Rule 5 — `limit` merge.
-///
-/// Mergeable iff both limits are absent. If either has a limit, refuse
-/// (broadening would mask the limit's intent).
-fn rule5_limit(a: &InterestShape, b: &InterestShape) -> bool {
-    a.limit.is_none() && b.limit.is_none()
-}
-
-/// Rule 6 — `lifecycle` merge.
-///
-/// Tailing and one-shot must not merge (one-shot would never close the tailing
-/// subscription). Both lifecycles must be identical.
-fn rule6_lifecycle(a: &InterestLifecycle, b: &InterestLifecycle) -> bool {
-    a == b
-}
-
-/// Rule 7 — `event_ids` merge by union.
-///
-/// Returns `None` if the union would exceed `limit`.
-fn rule7_event_ids(
-    a: &InterestShape,
-    b: &InterestShape,
-    limit: usize,
-) -> Option<std::collections::BTreeSet<super::interest::EventId>> {
-    let union: std::collections::BTreeSet<_> = a.event_ids.union(&b.event_ids).cloned().collect();
-    if union.len() > limit {
-        None
-    } else {
-        Some(union)
-    }
-}
-
-/// Rule 8 — `addresses` merge by union.
-///
-/// Merges the address-pointer sets. Returns `None` if the union exceeds `limit`.
-/// The other constraints (authors, kinds, time, lifecycle) must have been
-/// checked by Rules 1–7 before reaching this point — the method does not
-/// re-check them.
-fn rule8_addresses(
-    a: &InterestShape,
-    b: &InterestShape,
-    limit: usize,
-) -> Option<std::collections::BTreeSet<super::interest::NaddrCoord>> {
-    let union: std::collections::BTreeSet<_> =
-        a.addresses.union(&b.addresses).cloned().collect();
-    if union.len() > limit {
-        None
-    } else {
-        Some(union)
-    }
-}
-
 // ─── Tests ───────────────────────────────────────────────────────────────────
 
 #[cfg(test)]
@@ -261,11 +151,47 @@ mod tests {
 
     #[test]
     fn rule1_wildcard_absorbs_specific() {
-        // a is wildcard (empty), b is specific — result is b's kinds
+        // a is wildcard (empty), b is specific — result MUST be wildcard (empty),
+        // NOT b.kinds. Returning b.kinds would narrow the merged subscription,
+        // causing the relay to miss kinds that the wildcard side intended to match.
         let a = InterestShape::default(); // kinds = empty (wildcard)
         let b = shape_with_kinds(&[1, 6]);
         let r = merge(&a, &b, &tailing(), &tailing());
-        assert!(matches!(r, MergeOutcome::Merged(ref s) if s.kinds == b.kinds));
+        assert!(
+            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
+            "wildcard ∪ {{1,6}} must be wildcard (empty set), not {{1,6}}"
+        );
+    }
+
+    #[test]
+    fn wildcard_unions_with_anything_stays_wildcard() {
+        // Negative-direction: wildcard merged with ANY concrete set must stay wildcard.
+        // This is the correctness test the T30 codex review flagged as missing.
+        let wildcard = InterestShape::default(); // kinds = empty
+        for concrete_kinds in [
+            vec![1u32],
+            vec![6],
+            vec![1, 6],
+            vec![0, 1, 3, 4, 5, 6, 7, 9, 10, 30023],
+        ] {
+            let concrete = shape_with_kinds(&concrete_kinds);
+            let r_ab = merge(&wildcard, &concrete, &tailing(), &tailing());
+            let r_ba = merge(&concrete, &wildcard, &tailing(), &tailing());
+            assert!(
+                matches!(r_ab, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
+                "wildcard ∪ {:?} must be wildcard (a=wildcard)", concrete_kinds
+            );
+            assert!(
+                matches!(r_ba, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
+                "wildcard ∪ {:?} must be wildcard (b=wildcard)", concrete_kinds
+            );
+        }
+        // wildcard ∪ wildcard = wildcard
+        let r = merge(&wildcard, &wildcard, &tailing(), &tailing());
+        assert!(
+            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
+            "wildcard ∪ wildcard must be wildcard"
+        );
     }
 
     // ── Rule 2 — tags ────────────────────────────────────────────────────────
@@ -292,7 +218,7 @@ mod tests {
     fn rule2_different_tag_dimensions_refuse() {
         let mut tags_a = BTreeMap::new();
         tags_a.insert("t".to_string(), ["bitcoin".to_string()].into_iter().collect::<BTreeSet<_>>());
-        let tags_b = BTreeMap::new(); // no #t dimension
+        let tags_b = BTreeMap::new();
         let a = InterestShape { tags: tags_a, ..Default::default() };
         let b = InterestShape { tags: tags_b, ..Default::default() };
         assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
@@ -400,7 +326,6 @@ mod tests {
 
     #[test]
     fn rule7_event_ids_cap_refuse() {
-        // Build two sets whose union exceeds DEFAULT_VALUE_LIMIT (1000)
         let ids_a: BTreeSet<String> = (0u32..600).map(|i| format!("{i:064x}")).collect();
         let ids_b: BTreeSet<String> = (500u32..1100).map(|i| format!("{i:064x}")).collect();
         let a = InterestShape { event_ids: ids_a, ..Default::default() };
@@ -412,16 +337,8 @@ mod tests {
 
     #[test]
     fn rule8_address_union_merges() {
-        let coord_a = NaddrCoord {
-            pubkey: "a".repeat(64),
-            kind: 30023,
-            d_tag: "post-a".to_string(),
-        };
-        let coord_b = NaddrCoord {
-            pubkey: "b".repeat(64),
-            kind: 30023,
-            d_tag: "post-b".to_string(),
-        };
+        let coord_a = NaddrCoord { pubkey: "a".repeat(64), kind: 30023, d_tag: "post-a".to_string() };
+        let coord_b = NaddrCoord { pubkey: "b".repeat(64), kind: 30023, d_tag: "post-b".to_string() };
         let a = InterestShape {
             kinds: [30023].into_iter().collect(),
             addresses: [coord_a.clone()].into_iter().collect(),
@@ -443,12 +360,7 @@ mod tests {
 
     #[test]
     fn rule8_address_dedup_identical_coord() {
-        // Two interests for the exact same NaddrCoord should merge into one.
-        let coord = NaddrCoord {
-            pubkey: "a".repeat(64),
-            kind: 30023,
-            d_tag: "my-post".to_string(),
-        };
+        let coord = NaddrCoord { pubkey: "a".repeat(64), kind: 30023, d_tag: "my-post".to_string() };
         let a = InterestShape {
             kinds: [30023].into_iter().collect(),
             addresses: [coord.clone()].into_iter().collect(),
@@ -457,7 +369,6 @@ mod tests {
         let b = a.clone();
         let r = merge(&a, &b, &one_shot(), &one_shot());
         if let MergeOutcome::Merged(s) = r {
-            // BTreeSet deduplicates; should still be one coord.
             assert_eq!(s.addresses.len(), 1);
         } else {
             panic!("expected Merged");
@@ -466,12 +377,7 @@ mod tests {
 
     #[test]
     fn rule8_addresses_respect_other_rules() {
-        // If lifecycle differs, Rule 6 fires first — addresses are irrelevant.
-        let coord = NaddrCoord {
-            pubkey: "a".repeat(64),
-            kind: 30023,
-            d_tag: "post".to_string(),
-        };
+        let coord = NaddrCoord { pubkey: "a".repeat(64), kind: 30023, d_tag: "post".to_string() };
         let a = InterestShape {
             kinds: [30023].into_iter().collect(),
             addresses: [coord.clone()].into_iter().collect(),
diff --git a/crates/nmp-core/src/planner/lattice/rules.rs b/crates/nmp-core/src/planner/lattice/rules.rs
new file mode 100644
index 0000000..15d41b3
--- /dev/null
+++ b/crates/nmp-core/src/planner/lattice/rules.rs
@@ -0,0 +1,136 @@
+//! Individual merge rule implementations for the filter-merge lattice.
+//!
+//! Each function corresponds to one rule from compiler.md §3.3.
+//! All rules are `pub(super)` — only the lattice `merge()` entry point is public.
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3.3
+//! Doctrine: D8 (zero per-event allocs on the hot path after warmup).
+
+use crate::planner::interest::{InterestLifecycle, InterestShape, NaddrCoord};
+
+/// Rule 1 — `kinds` merge.
+///
+/// Mergeable iff `a.kinds == b.kinds` OR one is empty (wildcard absorbs ALL).
+///
+/// An empty set means "match any kind" (wildcard). When either side is wildcard,
+/// the result MUST be wildcard (empty), not the other side's concrete set.
+/// Returning the concrete set would NARROW the subscription semantics — a relay
+/// receiving `{ kinds: [1, 6] }` would miss kinds 0, 30023, etc. that the
+/// wildcard side intended to include.
+///
+/// `wildcard ∪ {1, 6} = wildcard` — the wildcard absorbs its neighbour.
+pub(super) fn rule1_kinds(
+    a: &InterestShape,
+    b: &InterestShape,
+) -> Option<std::collections::BTreeSet<u32>> {
+    if a.kinds.is_empty() || b.kinds.is_empty() {
+        // At least one side is wildcard — wildcard absorbs, result is wildcard.
+        Some(std::collections::BTreeSet::new())
+    } else if a.kinds == b.kinds {
+        Some(a.kinds.clone())
+    } else {
+        // Both non-empty but different — refuse (merging would widen kinds)
+        None
+    }
+}
+
+/// Rule 2 — `tags` merge.
+///
+/// Mergeable iff both shapes have the same tag key dimensions, AND the union
+/// of values per dimension stays under `limit`.
+pub(super) fn rule2_tags(
+    a: &InterestShape,
+    b: &InterestShape,
+    limit: usize,
+) -> Option<std::collections::BTreeMap<crate::planner::interest::TagKey, std::collections::BTreeSet<String>>> {
+    // Keys must be identical (same dimensions)
+    if a.tags.keys().ne(b.tags.keys()) {
+        return None;
+    }
+
+    let mut merged = std::collections::BTreeMap::new();
+    for (key, av) in &a.tags {
+        let bv = b.tags.get(key)?;
+        let union: std::collections::BTreeSet<String> = av.union(bv).cloned().collect();
+        if union.len() > limit {
+            return None;
+        }
+        merged.insert(key.clone(), union);
+    }
+    Some(merged)
+}
+
+/// Rule 3 — `since` merge.
+///
+/// Returns `min(a.since, b.since)` iff both are `Some` or both are `None`.
+/// Mixed (one bounded, one unbounded) returns `None` (refuse).
+pub(super) fn rule3_since(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
+    match (a.since, b.since) {
+        (None, None) => Some(None),
+        (Some(sa), Some(sb)) => Some(Some(sa.min(sb))),
+        _ => None,
+    }
+}
+
+/// Rule 4 — `until` merge.
+///
+/// Returns `max(a.until, b.until)` iff both are `Some` or both are `None`.
+/// Mixed returns `None` (refuse).
+pub(super) fn rule4_until(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
+    match (a.until, b.until) {
+        (None, None) => Some(None),
+        (Some(ua), Some(ub)) => Some(Some(ua.max(ub))),
+        _ => None,
+    }
+}
+
+/// Rule 5 — `limit` merge.
+///
+/// Mergeable iff both limits are absent. If either has a limit, refuse
+/// (broadening would mask the limit's intent).
+pub(super) fn rule5_limit(a: &InterestShape, b: &InterestShape) -> bool {
+    a.limit.is_none() && b.limit.is_none()
+}
+
+/// Rule 6 — `lifecycle` merge.
+///
+/// Tailing and one-shot must not merge (one-shot would never close the tailing
+/// subscription). Both lifecycles must be identical.
+pub(super) fn rule6_lifecycle(a: &InterestLifecycle, b: &InterestLifecycle) -> bool {
+    a == b
+}
+
+/// Rule 7 — `event_ids` merge by union.
+///
+/// Returns `None` if the union would exceed `limit`.
+pub(super) fn rule7_event_ids(
+    a: &InterestShape,
+    b: &InterestShape,
+    limit: usize,
+) -> Option<std::collections::BTreeSet<crate::planner::interest::EventId>> {
+    let union: std::collections::BTreeSet<_> = a.event_ids.union(&b.event_ids).cloned().collect();
+    if union.len() > limit {
+        None
+    } else {
+        Some(union)
+    }
+}
+
+/// Rule 8 — `addresses` merge by union.
+///
+/// Merges the address-pointer sets. Returns `None` if the union exceeds `limit`.
+/// The other constraints (authors, kinds, time, lifecycle) must have been
+/// checked by Rules 1–7 before reaching this point.
+pub(super) fn rule8_addresses(
+    a: &InterestShape,
+    b: &InterestShape,
+    limit: usize,
+) -> Option<std::collections::BTreeSet<NaddrCoord>> {
+    let union: std::collections::BTreeSet<_> =
+        a.addresses.union(&b.addresses).cloned().collect();
+    if union.len() > limit {
+        None
+    } else {
+        Some(union)
+    }
+}
diff --git a/crates/nmp-core/src/planner/mod.rs b/crates/nmp-core/src/planner/mod.rs
index 72f4abc..4f30b83 100644
--- a/crates/nmp-core/src/planner/mod.rs
+++ b/crates/nmp-core/src/planner/mod.rs
@@ -34,20 +34,35 @@
 //!
 //! Design: `docs/design/subscription-compilation/`
 
-pub mod compiler;
-pub mod interest;
-pub mod lattice;
-pub mod plan;
+pub(crate) mod compiler;
+pub(crate) mod interest;
+pub(crate) mod lattice;
+pub(crate) mod plan;
 
-// ─── Convenience re-exports ──────────────────────────────────────────────────
+// ─── Public API surface ──────────────────────────────────────────────────────
+//
+// Only the items below cross the crate boundary. Internals (RelayEntry,
+// partition_interest, FnvHasher, rule*_* functions, etc.) stay module-private.
+// `lattice::merge` is re-exported for the nmp-testing audit gate; all others
+// are consumed by crate-internal callers (kernel, actor).
 
 pub use compiler::{
-    EmptyMailboxCache, InMemoryMailboxCache, MailboxCache, MailboxSnapshot,
+    CompileContext,
+    EmptyMailboxCache,
+    InMemoryMailboxCache,
+    MailboxCache,
+    MailboxSnapshot,
     SubscriptionCompiler,
 };
 pub use interest::{
-    EventId, HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape,
-    LogicalInterest, NaddrCoord, Pubkey, RelayHint, RelayUrl, TagKey, UnixSeconds,
+    InterestId,
+    InterestLifecycle,
+    InterestScope,
+    InterestShape,
+    LogicalInterest,
+    NaddrCoord,
+    Pubkey,
+    RelayUrl,
 };
-pub use lattice::{merge, MergeOutcome};
-pub use plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape};
+pub use lattice::MergeOutcome;
+pub use plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape, UserConfiguredCategory};
diff --git a/crates/nmp-core/src/planner/plan.rs b/crates/nmp-core/src/planner/plan.rs
index b91a060..26458f5 100644
--- a/crates/nmp-core/src/planner/plan.rs
+++ b/crates/nmp-core/src/planner/plan.rs
@@ -9,6 +9,37 @@ use std::collections::{BTreeMap, BTreeSet};
 
 use super::interest::{InterestId, InterestShape, RelayUrl};
 
+// ─── UserConfiguredCategory ──────────────────────────────────────────────────
+
+/// Sub-category for `RoutingSource::UserConfigured`.
+///
+/// Indexer fallback is NOT a fifth diagnostic lane — it is `UserConfigured`
+/// with sub-category `Indexer`. This preserves the four-lane discipline
+/// (`docs/design/subscription-compilation/diagnostics.md` §5.0 + §5.1 Lane 4)
+/// so the diagnostic UI always sees exactly four columns regardless of whether
+/// an author is served via NIP-65, hints, provenance, or any user-configured
+/// sub-category.
+///
+/// `ByLaneCounts::indexer_fallback` in the coverage view exposes the indexer
+/// sub-count WITHOUT splitting lane 4 — it is a sub-count of `user_configured`,
+/// not an extra lane.
+#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
+pub enum UserConfiguredCategory {
+    /// User's own read relays (overrides NIP-65 read for the active account).
+    AccountRead,
+    /// User's own write relays.
+    AccountWrite,
+    /// Kernel-configured indexer relay (e.g. purplepag.es).
+    ///
+    /// This is the sub-category that represents indexer fallback routing in
+    /// diagnostics — it is lane 4 (User-configured), not a fifth lane. The
+    /// indexer set is an operator policy choice applied by the kernel when
+    /// NIP-65 mailboxes are unknown. Never used for writes (D3).
+    Indexer,
+    /// Operator-injected relay for debug/testing purposes.
+    Debug,
+}
+
 // ─── RoutingSource ───────────────────────────────────────────────────────────
 
 /// Why a relay was included in the plan.
@@ -18,19 +49,26 @@ use super::interest::{InterestId, InterestShape, RelayUrl};
 /// preserving all reasons — the four-lane diagnostic discipline requires that
 /// lanes are never collapsed.
 ///
+/// **Indexer fallback** is represented as `UserConfigured(UserConfiguredCategory::Indexer)`,
+/// NOT as a separate variant. There are exactly four lanes in the diagnostic model
+/// (NIP-65 / Hint / Provenance / User-configured); the indexer is a sub-category
+/// of lane 4. See `docs/design/subscription-compilation/diagnostics.md` §5.0.
+///
 /// Design: `docs/design/subscription-compilation/diagnostics.md` §5.2
 #[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
 pub enum RoutingSource {
-    /// Resolved from the author's published kind:10002 relay list.
+    /// Resolved from the author's published kind:10002 relay list (lane 1).
     Nip65,
-    /// Resolved from a user-configured relay set.
-    UserConfigured,
-    /// Resolved from indexer fallback (no mailbox known for the author).
-    Indexer,
-    /// Resolved from a routing hint embedded in an event tag.
+    /// Resolved from a routing hint embedded in an event tag (lane 2).
     Hint,
-    /// Observed as the provenance relay for a prior event.
+    /// Observed as the provenance relay for a prior event (lane 3).
     Provenance,
+    /// Resolved from a user-configured or operator-policy relay set (lane 4).
+    ///
+    /// Includes indexer fallback as `UserConfigured(UserConfiguredCategory::Indexer)`.
+    /// The sub-category is carried here so that `RelayPlan::role_tags` remains
+    /// self-describing without consulting a separate fact stream.
+    UserConfigured(UserConfiguredCategory),
 }
 
 // ─── SubShape ────────────────────────────────────────────────────────────────
@@ -41,7 +79,7 @@ pub enum RoutingSource {
 /// frame. The `canonical_filter_hash` provides stable identity for ADR-0007
 /// `WireSubscriptionStatus` records across re-emissions.
 ///
-/// # TODO(wire-emitter)
+/// # Wire-emitter lifecycle field
 /// Add `lifecycle: InterestLifecycle` to this struct when the wire-emitter lands.
 /// The compiler already computes lifecycle during the Stage 3 greedy merge;
 /// lifecycle equality is enforced by Rule 6 before any two shapes are merged.
diff --git a/crates/nmp-testing/tests/m2_plan_id_stability.rs b/crates/nmp-testing/tests/m2_plan_id_stability.rs
new file mode 100644
index 0000000..eea6a19
--- /dev/null
+++ b/crates/nmp-testing/tests/m2_plan_id_stability.rs
@@ -0,0 +1,225 @@
+//! M2 plan-id stability tests: §3.4 "referenced-pubkeys only" invariant.
+//!
+//! These tests verify that the plan-id hash covers ONLY the pubkeys that are
+//! referenced by the interest set (authors, #p tags, address pubkeys), not the
+//! entire mailbox cache. This was the core bug in the T26 implementation.
+//!
+//! Split from `m2_subscription_compilation_audit.rs` for the 500-LOC limit.
+//!
+//! CI gate: `cargo test -p nmp-testing --test m2_plan_id_stability`
+//!
+//! Design: `docs/design/subscription-compilation/compiler.md` §3.4
+//! Doctrine: D8 (plan-id stability avoids redundant recompilation).
+
+use nmp_core::planner::{
+    CompileContext,
+    InMemoryMailboxCache,
+    MailboxSnapshot,
+    SubscriptionCompiler,
+    InterestId,
+    InterestLifecycle,
+    InterestScope,
+    InterestShape,
+    LogicalInterest,
+};
+
+// ─── Helpers (duplicated from audit to keep files independent) ────────────────
+
+fn pubkey(seed: &str) -> String {
+    format!("{seed:0>64}")
+        .chars()
+        .take(64)
+        .collect::<String>()
+        .to_lowercase()
+}
+
+fn relay(url: &str) -> String {
+    url.to_string()
+}
+
+fn interest_id(n: u64) -> InterestId {
+    InterestId(n)
+}
+
+// ─── Plan-id stability: unrelated mailbox arrival ────────────────────────────
+
+/// An unrelated kind:10002 arrival — for a pubkey NOT in any interest's
+/// author set, #p tags, or address pubkeys — MUST NOT change the plan-id.
+///
+/// This tests the §3.4 "referenced-pubkeys only" invariant that was violated
+/// by the original T26 implementation (which hashed the ENTIRE mailbox cache).
+///
+/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
+#[test]
+fn plan_id_unchanged_when_unrelated_mailbox_arrives() {
+    let alice_pk = pubkey("alice");
+    let unrelated_pk = pubkey("unrelated_stranger");
+
+    let mut cache = InMemoryMailboxCache::new();
+    cache.put(
+        alice_pk.clone(),
+        MailboxSnapshot {
+            write_relays: vec![relay("wss://alice.example")],
+            read_relays: vec![],
+            both_relays: vec![],
+        },
+    );
+
+    let indexer = vec![relay("wss://purplepag.es")];
+    let ctx = CompileContext::default();
+
+    let interest = LogicalInterest {
+        id: interest_id(1),
+        scope: InterestScope::Global,
+        shape: InterestShape {
+            authors: [alice_pk.clone()].into_iter().collect(),
+            kinds: [1u32, 6u32].into_iter().collect(),
+            ..Default::default()
+        },
+        hints: vec![],
+        lifecycle: InterestLifecycle::Tailing,
+    };
+
+    let plan_before = {
+        let compiler = SubscriptionCompiler::new(&cache, &indexer);
+        compiler
+            .compile_with_context(std::slice::from_ref(&interest), &ctx)
+            .expect("compile before")
+    };
+
+    // A kind:10002 arrives for unrelated_stranger — NOT in the interest's author set.
+    cache.put(
+        unrelated_pk.clone(),
+        MailboxSnapshot {
+            write_relays: vec![relay("wss://stranger.example")],
+            read_relays: vec![],
+            both_relays: vec![],
+        },
+    );
+
+    let plan_after = {
+        let compiler = SubscriptionCompiler::new(&cache, &indexer);
+        compiler
+            .compile_with_context(std::slice::from_ref(&interest), &ctx)
+            .expect("compile after")
+    };
+
+    assert_eq!(
+        plan_before.plan_id, plan_after.plan_id,
+        "unrelated mailbox arrival (pubkey not in interest set) must NOT change plan_id"
+    );
+}
+
+// ─── Plan-id stability: referenced author mailbox update ─────────────────────
+
+/// A kind:10002 update for a pubkey that IS in the interest's author set
+/// MUST change the plan-id (the compiler must re-route).
+///
+/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
+#[test]
+fn plan_id_changes_when_referenced_author_mailbox_updates() {
+    let alice_pk = pubkey("alice");
+
+    let mut cache = InMemoryMailboxCache::new();
+    cache.put(
+        alice_pk.clone(),
+        MailboxSnapshot {
+            write_relays: vec![relay("wss://alice-old.example")],
+            read_relays: vec![],
+            both_relays: vec![],
+        },
+    );
+
+    let indexer = vec![relay("wss://purplepag.es")];
+    let ctx = CompileContext::default();
+
+    let interest = LogicalInterest {
+        id: interest_id(1),
+        scope: InterestScope::Global,
+        shape: InterestShape {
+            authors: [alice_pk.clone()].into_iter().collect(),
+            kinds: [1u32, 6u32].into_iter().collect(),
+            ..Default::default()
+        },
+        hints: vec![],
+        lifecycle: InterestLifecycle::Tailing,
+    };
+
+    let plan_before = {
+        let compiler = SubscriptionCompiler::new(&cache, &indexer);
+        compiler
+            .compile_with_context(std::slice::from_ref(&interest), &ctx)
+            .expect("compile before")
+    };
+
+    // Alice publishes a new kind:10002 pointing to a different relay.
+    cache.put(
+        alice_pk.clone(),
+        MailboxSnapshot {
+            write_relays: vec![relay("wss://alice-new.example")],
+            read_relays: vec![],
+            both_relays: vec![],
+        },
+    );
+
+    let plan_after = {
+        let compiler = SubscriptionCompiler::new(&cache, &indexer);
+        compiler
+            .compile_with_context(std::slice::from_ref(&interest), &ctx)
+            .expect("compile after")
+    };
+
+    assert_ne!(
+        plan_before.plan_id, plan_after.plan_id,
+        "mailbox update for a referenced author MUST change plan_id"
+    );
+}
+
+// ─── Plan-id stability: indexer set version bump ────────────────────────────
+
+/// Bumping `indexer_set_version` in the compile context must change plan-id
+/// even when the interest set and mailbox cache are identical.
+///
+/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
+#[test]
+fn plan_id_changes_on_indexer_set_version_bump() {
+    let mut cache = InMemoryMailboxCache::new();
+    cache.put(
+        pubkey("alice"),
+        MailboxSnapshot {
+            write_relays: vec![relay("wss://alice.example")],
+            read_relays: vec![],
+            both_relays: vec![],
+        },
+    );
+
+    let indexer = vec![relay("wss://purplepag.es")];
+    let compiler = SubscriptionCompiler::new(&cache, &indexer);
+
+    let interest = LogicalInterest {
+        id: interest_id(1),
+        scope: InterestScope::Global,
+        shape: InterestShape {
+            authors: [pubkey("alice")].into_iter().collect(),
+            kinds: [1u32, 6u32].into_iter().collect(),
+            ..Default::default()
+        },
+        hints: vec![],
+        lifecycle: InterestLifecycle::Tailing,
+    };
+
+    let ctx_v0 = CompileContext { indexer_set_version: 0, user_config_version: 0 };
+    let ctx_v1 = CompileContext { indexer_set_version: 1, user_config_version: 0 };
+
+    let plan_v0 = compiler
+        .compile_with_context(std::slice::from_ref(&interest), &ctx_v0)
+        .expect("compile v0");
+    let plan_v1 = compiler
+        .compile_with_context(std::slice::from_ref(&interest), &ctx_v1)
+        .expect("compile v1");
+
+    assert_ne!(
+        plan_v0.plan_id, plan_v1.plan_id,
+        "indexer_set_version bump MUST change plan_id"
+    );
+}
diff --git a/crates/nmp-testing/tests/m2_subscription_compilation_audit.rs b/crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
index b7aa5ca..09b0451 100644
--- a/crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
+++ b/crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
@@ -13,12 +13,20 @@
 //! Design: `docs/design/subscription-compilation/tests.md`
 //! Doctrine: D3 (routing automatic), D6 (errors internal), D8 (zero allocs).
 
+// Import through the planner's public API surface — submodule paths are
+// pub(crate) and must not be named from an external crate.
 use nmp_core::planner::{
-    compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
-    interest::{
-        InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest, NaddrCoord,
-    },
-    plan::RoutingSource,
+    InMemoryMailboxCache,
+    MailboxSnapshot,
+    SubscriptionCompiler,
+    InterestId,
+    InterestLifecycle,
+    InterestScope,
+    InterestShape,
+    LogicalInterest,
+    NaddrCoord,
+    RoutingSource,
+    UserConfiguredCategory,
 };
 use std::collections::BTreeSet;
 
@@ -84,7 +92,8 @@ fn interest_id(n: u64) -> InterestId {
 /// Design: `docs/design/subscription-compilation/tests.md` §9.2 Assertion 2.
 #[test]
 fn timeline_compiles_to_per_relay_union() {
-    let authors = make_authors_with_overlapping_mailboxes(300);
+    // Design spec §9.2 Assertion 2 states 1000 authors as the boundary.
+    let authors = make_authors_with_overlapping_mailboxes(1000);
 
     let mut cache = InMemoryMailboxCache::new();
     for (pk, mb) in &authors {
@@ -222,6 +231,29 @@ fn address_pointer_dedup_across_two_interests() {
         1,
         "Rule 8 must merge identical address sets into one SubShape"
     );
+
+    // Assert: merged sub-shape contains the union of both interests' address sets.
+    // (Both interests had the same coord, so the union is that one coord.)
+    let sub = &relay_plan.sub_shapes[0];
+    assert!(
+        sub.shape.addresses.contains(&coord),
+        "merged SubShape must contain the NaddrCoord from both interests"
+    );
+    assert_eq!(
+        sub.shape.addresses.len(),
+        1,
+        "dedup: merged address set must have exactly one coord (union of two identical sets)"
+    );
+
+    // Assert: both originating interests are tracked in the merged plan output.
+    // D8 invariant: the reverse index must account for all claim holders.
+    let mut tracked_ids: Vec<u64> = sub.originating_interests.iter().map(|id| id.0).collect();
+    tracked_ids.sort();
+    assert_eq!(
+        tracked_ids,
+        vec![10, 11],
+        "both originating InterestIds must be recorded in the merged SubShape"
+    );
 }
 
 // ─── Fallback to indexer set when no mailbox known ───────────────────────────
@@ -253,9 +285,11 @@ fn unknown_author_falls_back_to_indexer() {
         "unknown author must fall back to indexer relay"
     );
     let rp = &plan.per_relay["wss://purplepag.es"];
+    // Indexer fallback is lane 4 (UserConfigured/Indexer) per diagnostics.md §5.0 + ADR-0007.
+    // There is NO RoutingSource::Indexer variant — indexer is a subcategory of UserConfigured.
     assert!(
-        rp.role_tags.contains(&RoutingSource::Indexer),
-        "routing source for indexer fallback must be Indexer"
+        rp.role_tags.contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Indexer)),
+        "routing source for indexer fallback must be UserConfigured(Indexer), not a fifth lane"
     );
 }
 
@@ -421,3 +455,6 @@ fn plan_id_changes_when_interest_set_changes() {
         "adding an interest must change plan_id even if wire REQs merge"
     );
 }
+
+// Plan-id referenced-pubkeys-only tests live in m2_plan_id_stability.rs
+// to keep this file under the 500-LOC hard limit.

Review for each:
- Did the agent address each numbered issue from their brief?
- Any new TODO/FIXME/unimplemented sneaking in?
- File sizes (any > 300 LOC soft / 500 hard)?
- Doctrine D0-D8 citations correct?
- For T29 specifically: M1 firehose-bench regression check passed (look for evidence in commit message or test files)?
- For T30 specifically: Rule 1 wildcard test added that exercises wildcard ∪ kind-set = wildcard?

Be terse. If both fine, say so. If concerns, name file:line + fix per commit.
codex
I’ll review this against the actual checked-out commits, not just the pasted diff, and I’ll do a quick memory pass first because this repo has recurring governance/doctrine context.
exec
/bin/zsh -lc 'rg -n "nostr-multi-platform|NMP|T29|T30|overview-and-dx|D0-D8" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
 exited 1 in 0ms:
codex
No relevant memory hit for this repo/task, so I’m moving straight to the local commits and current tree. I’m checking the merge snapshots, file sizes, TODO markers, and the specific behavioral/test claims.
exec
/bin/zsh -lc 'git status --short --branch' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
## master...origin/master
?? docs/perf/firehose-bench/1779069727-live.json
?? docs/perf/firehose-bench/1779069727-live.md

exec
/bin/zsh -lc 'git rev-parse --verify 7d16b3f && git rev-parse --verify bc84cfe && git log --oneline --decorate -5' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
7d16b3f248c0df396c741323b99d09bf5a9b48e3
bc84cfe4b76fcf3c1bfeb1456fffc25634181036
bc84cfe (HEAD -> master, origin/master) fix(m2): codex follow-up — Rule 1 wildcard + plan-id stability + lane modeling + file splits + TODO removal (T30)
c3bb2fb design(m2): add addresses: BTreeSet<NaddrCoord> to InterestShape (T21 substrate enhancement)
7d16b3f fix(m3): codex follow-up — mem.rs split + sig verify + ingest wired + GC ceiling + tombstones (T29)
0c13aa1 perf(orchestration): HB18 — T29+T30 heavily active; T31 held on T29 dep
b370eea perf(orchestration): HB17 — T30 dispatched (Rule 1 wildcard fix); T29+T30 in flight; T31 held

exec
/bin/zsh -lc 'git show --stat --oneline --decorate --no-renames 7d16b3f -- && git show --stat --oneline --decorate --no-renames bc84cfe --' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
7d16b3f fix(m3): codex follow-up — mem.rs split + sig verify + ingest wired + GC ceiling + tombstones (T29)
 Cargo.lock                                    |  603 ++++++++++++++
 crates/nmp-core/Cargo.toml                    |    1 +
 crates/nmp-core/src/kernel/ingest.rs          |   60 +-
 crates/nmp-core/src/kernel/mod.rs             |   11 +-
 crates/nmp-core/src/kernel/nostr.rs           |    4 +
 crates/nmp-core/src/store/events.rs           |    9 +-
 crates/nmp-core/src/store/lmdb.rs             |    6 +-
 crates/nmp-core/src/store/mem.rs              | 1105 -------------------------
 crates/nmp-core/src/store/mem/domain.rs       |   79 ++
 crates/nmp-core/src/store/mem/gc.rs           |  194 +++++
 crates/nmp-core/src/store/mem/insert.rs       |  357 ++++++++
 crates/nmp-core/src/store/mem/mod.rs          |  193 +++++
 crates/nmp-core/src/store/mem/query.rs        |  394 +++++++++
 crates/nmp-core/src/store/mem/store_impl.rs   |  181 ++++
 crates/nmp-core/src/store/mem/tests.rs        |  132 +++
 crates/nmp-core/src/store/mod.rs              |    6 +-
 crates/nmp-core/src/store/types.rs            |  343 --------
 crates/nmp-core/src/store/types/errors.rs     |   80 ++
 crates/nmp-core/src/store/types/events.rs     |  188 +++++
 crates/nmp-core/src/store/types/gc.rs         |   67 ++
 crates/nmp-core/src/store/types/ids.rs        |   36 +
 crates/nmp-core/src/store/types/mod.rs        |   20 +
 crates/nmp-core/src/store/types/outcomes.rs   |   68 ++
 crates/nmp-core/src/store/types/watermark.rs  |   43 +
 crates/nmp-testing/src/store_harness.rs       |    8 +-
 docs/perf/firehose-bench/1779068929-live.json |   74 ++
 docs/perf/firehose-bench/1779068929-live.md   |   21 +
 docs/perf/firehose-bench/1779070804-live.json |   74 ++
 docs/perf/firehose-bench/1779070804-live.md   |   21 +
 29 files changed, 2906 insertions(+), 1472 deletions(-)
bc84cfe (HEAD -> master, origin/master) fix(m2): codex follow-up — Rule 1 wildcard + plan-id stability + lane modeling + file splits + TODO removal (T30)
 crates/nmp-core/src/kernel/requests/profile.rs     |   2 +-
 crates/nmp-core/src/kernel/requests/thread.rs      |   2 +-
 crates/nmp-core/src/planner/compiler.rs            | 498 ---------------------
 crates/nmp-core/src/planner/compiler/mailbox.rs    | 105 +++++
 crates/nmp-core/src/planner/compiler/mod.rs        | 191 ++++++++
 crates/nmp-core/src/planner/compiler/partition.rs  | 258 +++++++++++
 crates/nmp-core/src/planner/compiler/plan_id.rs    | 159 +++++++
 crates/nmp-core/src/planner/interest.rs            |   4 +-
 crates/nmp-core/src/planner/lattice.rs             | 483 --------------------
 crates/nmp-core/src/planner/lattice/mod.rs         | 389 ++++++++++++++++
 crates/nmp-core/src/planner/lattice/rules.rs       | 136 ++++++
 crates/nmp-core/src/planner/mod.rs                 |  35 +-
 crates/nmp-core/src/planner/plan.rs                |  54 ++-
 crates/nmp-testing/tests/m2_plan_id_stability.rs   | 225 ++++++++++
 .../tests/m2_subscription_compilation_audit.rs     |  53 ++-
 15 files changed, 1583 insertions(+), 1011 deletions(-)

codex
The branch is at `bc84cfe` on `master`. There are two untracked firehose bench files in `docs/perf/firehose-bench/`, which I’m ignoring for the review because they are not part of either commit.
exec
/bin/zsh -lc 'git diff-tree --no-commit-id --name-only -r 7d16b3f | while IFS= read -r f; do if git cat-file -e 7d16b3f:"$f" 2>/dev/null; then n=$(git show 7d16b3f:"$f" | wc -l | tr -d '"' '); printf \"%5s %s\\n\" \""'$n" "$f"; fi; done | sort -nr' in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc 'git diff-tree --no-commit-id --name-only -r bc84cfe | while IFS= read -r f; do if git cat-file -e bc84cfe:"$f" 2>/dev/null; then n=$(git show bc84cfe:"$f" | wc -l | tr -d '"' '); printf \"%5s %s\\n\" \""'$n" "$f"; fi; done | sort -nr' in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "git diff-tree --no-commit-id --name-only -r 7d16b3f bc84cfe | sort -u | tr '\\n' '\\0' | xargs -0 -I{} sh -c 'git cat-file -e bc84cfe:\""'$1" 2>/dev/null && git grep -n -E "TODO|FIXME|unimplemented!|todo!|panic!'"\\\\(\\\\\\\"not implemented|not yet implemented\" bc84cfe -- \""'$1" || true'"' sh {}" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 338ms:
  460 crates/nmp-testing/tests/m2_subscription_compilation_audit.rs
  389 crates/nmp-core/src/planner/lattice/mod.rs
  258 crates/nmp-core/src/planner/compiler/partition.rs
  255 crates/nmp-core/src/kernel/requests/profile.rs
  225 crates/nmp-testing/tests/m2_plan_id_stability.rs
  225 crates/nmp-core/src/planner/interest.rs
  191 crates/nmp-core/src/planner/compiler/mod.rs
  174 crates/nmp-core/src/planner/plan.rs
  168 crates/nmp-core/src/kernel/requests/thread.rs
  159 crates/nmp-core/src/planner/compiler/plan_id.rs
  136 crates/nmp-core/src/planner/lattice/rules.rs
  105 crates/nmp-core/src/planner/compiler/mailbox.rs
   68 crates/nmp-core/src/planner/mod.rs

 succeeded in 434ms:
 succeeded in 684ms:
 1410 Cargo.lock
  427 crates/nmp-core/src/kernel/ingest.rs
  417 crates/nmp-core/src/kernel/mod.rs
  394 crates/nmp-core/src/store/mem/query.rs
  357 crates/nmp-core/src/store/mem/insert.rs
  296 crates/nmp-core/src/store/events.rs
  263 crates/nmp-testing/src/store_harness.rs
  230 crates/nmp-core/src/kernel/nostr.rs
  226 crates/nmp-core/src/store/lmdb.rs
  194 crates/nmp-core/src/store/mem/gc.rs
  193 crates/nmp-core/src/store/mem/mod.rs
  188 crates/nmp-core/src/store/types/events.rs
  181 crates/nmp-core/src/store/mem/store_impl.rs
  132 crates/nmp-core/src/store/mem/tests.rs
   80 crates/nmp-core/src/store/types/errors.rs
   79 crates/nmp-core/src/store/mem/domain.rs
   73 docs/perf/firehose-bench/1779070804-live.json
   73 docs/perf/firehose-bench/1779068929-live.json
   68 crates/nmp-core/src/store/types/outcomes.rs
   67 crates/nmp-core/src/store/types/gc.rs
   51 crates/nmp-core/src/store/mod.rs
   43 crates/nmp-core/src/store/types/watermark.rs
   36 crates/nmp-core/src/store/types/ids.rs
   24 crates/nmp-core/Cargo.toml
   21 docs/perf/firehose-bench/1779070804-live.md
   21 docs/perf/firehose-bench/1779068929-live.md
   20 crates/nmp-core/src/store/types/mod.rs

codex
The LOC pass already shows several soft-limit files remain (`kernel/ingest.rs`, `kernel/mod.rs`, `store/mem/query.rs`, T30 audit/lattice files), but none of the changed hand-authored files are over the 500 hard ceiling. I’m now checking whether the claimed fixes actually hold at code level.
exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/ingest.rs | sed -n '220,330p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/store/types/events.rs | sed -n '1,240p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   220	            .get(&event.pubkey)
   221	            .map(|current| relay_list.created_at >= current.created_at)
   222	            .unwrap_or(true);
   223	        if should_replace {
   224	            self.log(format!(
   225	                "NIP-65 {} read={} write={} both={}",
   226	                short_hex(&event.pubkey),
   227	                relay_list.read_relays.len(),
   228	                relay_list.write_relays.len(),
   229	                relay_list.both_relays.len()
   230	            ));
   231	            self.author_relay_lists.insert(event.pubkey, relay_list);
   232	        }
   233	    }
   234	
   235	    pub(super) fn ingest_timeline_event(&mut self, role: RelayRole, sub_id: &str, event: NostrEvent) {
   236	        // Duplicate check on the in-memory read-cache.
   237	        if self.events.contains_key(&event.id) {
   238	            if let Some(cached) = self.events.get_mut(&event.id) {
   239	                cached.relay_count = cached.relay_count.saturating_add(1);
   240	            }
   241	            return;
   242	        }
   243	
   244	        if !self.should_store_event(sub_id, &event) {
   245	            return;
   246	        }
   247	
   248	        // D4: route through EventStore (the single writer).
   249	        // Signature verification via VerifiedEvent::try_from_raw. Events that
   250	        // fail verification are logged and dropped — not cached locally.
   251	        let raw = crate::store::RawEvent {
   252	            id: event.id.clone(),
   253	            pubkey: event.pubkey.clone(),
   254	            created_at: event.created_at,
   255	            kind: event.kind,
   256	            tags: event.tags.clone(),
   257	            content: event.content.clone(),
   258	            sig: event.sig.clone(),
   259	        };
   260	        let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
   261	            Ok(v) => v,
   262	            Err(e) => {
   263	                self.log(format!("sig verify failed for {}: {e}", &event.id[..16]));
   264	                return;
   265	            }
   266	        };
   267	        let relay_url = role.url().to_string();
   268	        let received_at_ms = std::time::SystemTime::now()
   269	            .duration_since(std::time::UNIX_EPOCH)
   270	            .map(|d| d.as_millis() as u64)
   271	            .unwrap_or(0);
   272	        // Store insert; log but don't abort on error (graceful degradation).
   273	        match self.store.insert(verified, &relay_url, received_at_ms) {
   274	            Ok(outcome) => {
   275	                use crate::store::InsertOutcome;
   276	                match outcome {
   277	                    InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. } => {}
   278	                    InsertOutcome::Duplicate { .. } | InsertOutcome::Superseded { .. } => {
   279	                        // Store already has a valid version; still cache locally for timeline.
   280	                    }
   281	                    InsertOutcome::Tombstoned { .. } | InsertOutcome::Rejected { .. }
   282	                    | InsertOutcome::Ephemeral { .. } => {
   283	                        // Store rejected the event; skip populating local cache.
   284	                        return;
   285	                    }
   286	                }
   287	            }
   288	            Err(e) => {
   289	                self.log(format!("store insert error: {e}"));
   290	                // Graceful degradation: continue with local-cache-only path.
   291	            }
   292	        }
   293	
   294	        // Populate the lightweight read-cache for timeline ordering + display.
   295	        let cached = StoredEvent {
   296	            id: event.id.clone(),
   297	            author: event.pubkey.clone(),
   298	            kind: event.kind,
   299	            created_at: event.created_at,
   300	            tags: event.tags,
   301	            content: event.content,
   302	            relay_count: 1,
   303	        };
   304	        self.events.insert(event.id.clone(), cached);
   305	        if sub_id.starts_with("diag-firehose-") {
   306	            self.diagnostic_firehose_events = self.diagnostic_firehose_events.saturating_add(1);
   307	        }
   308	        self.enqueue_thread_hydration_from_event(&event.id);
   309	        if self.timeline_authors.contains(&event.pubkey) || sub_id.starts_with("diag-firehose-") {
   310	            self.timeline.push_back(event.id);
   311	            self.sort_timeline();
   312	            self.timeline_first_item_at.get_or_insert_with(Instant::now);
   313	        }
   314	    }
   315	
   316	    pub(super) fn should_store_event(&self, sub_id: &str, event: &NostrEvent) -> bool {
   317	        self.timeline_authors.contains(&event.pubkey)
   318	            || self
   319	                .selected_author
   320	                .as_ref()
   321	                .map(|interest| interest.key == event.pubkey)
   322	                .unwrap_or(false)
   323	            || sub_id.starts_with("author-notes-")
   324	            || sub_id.starts_with("thread-ids-")
   325	            || sub_id.starts_with("thread-replies-")
   326	            || sub_id.starts_with("diag-firehose-")
   327	    }
   328	
   329	    pub(super) fn enqueue_thread_hydration_from_event(&mut self, event_id: &str) {
   330	        let Some(selected) = self

 succeeded in 0ms:
     1	//! `RawEvent`, `VerifiedEvent`, and `StoredEvent` types.
     2	//!
     3	//! `VerifiedEvent` is the gate type for `EventStore::insert`: only events that
     4	//! have passed Schnorr signature verification can enter the store.
     5	
     6	use std::sync::Arc;
     7	use serde::{Deserialize, Serialize};
     8	use super::ids::{EventId, PubKey, hex_to_bytes32, hex_nibble};
     9	use super::errors::VerifyError;
    10	
    11	// ─── RawEvent ────────────────────────────────────────────────────────────────
    12	
    13	/// Temporary stand-in for `nostr::Event` until the nostr crate is in the workspace.
    14	///
    15	/// Fields match the NIP-01 event object exactly. Signature verification is
    16	/// skipped for now (insert always trusts the caller). The M3-lmdb task will
    17	/// swap this for the real type and enable proper sig checks.
    18	#[derive(Clone, Debug, Serialize, Deserialize)]
    19	pub struct RawEvent {
    20	    pub id: String,          // lowercase hex
    21	    pub pubkey: String,      // lowercase hex
    22	    pub created_at: u64,     // unix seconds
    23	    pub kind: u32,
    24	    pub tags: Vec<Vec<String>>,
    25	    pub content: String,
    26	    pub sig: String,         // lowercase hex
    27	}
    28	
    29	impl RawEvent {
    30	    /// Decode hex id → 32 bytes. Returns zeroes on malformed input.
    31	    pub fn id_bytes(&self) -> EventId {
    32	        hex_to_bytes32(&self.id)
    33	    }
    34	
    35	    /// Decode hex pubkey → 32 bytes. Returns zeroes on malformed input.
    36	    pub fn pubkey_bytes(&self) -> PubKey {
    37	        hex_to_bytes32(&self.pubkey)
    38	    }
    39	
    40	    /// NIP-01 replaceable kinds: 0, 3, and 10000–19999.
    41	    pub fn is_replaceable(&self) -> bool {
    42	        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
    43	    }
    44	
    45	    /// NIP-33 parameterized replaceable kinds: 30000–39999.
    46	    pub fn is_param_replaceable(&self) -> bool {
    47	        (30_000..40_000).contains(&self.kind)
    48	    }
    49	
    50	    /// NIP-16 ephemeral kinds: 20000–29999.
    51	    pub fn is_ephemeral(&self) -> bool {
    52	        (20_000..30_000).contains(&self.kind)
    53	    }
    54	
    55	    /// Returns the value of the first `d` tag, if present.
    56	    pub fn d_tag(&self) -> Option<Vec<u8>> {
    57	        self.tags
    58	            .iter()
    59	            .find(|t| t.first().map(|s| s == "d").unwrap_or(false))
    60	            .and_then(|t| t.get(1))
    61	            .map(|s| s.as_bytes().to_vec())
    62	    }
    63	
    64	    /// Returns the unix-second value of the first `expiration` tag, if present.
    65	    pub fn expiration(&self) -> Option<u64> {
    66	        self.tags
    67	            .iter()
    68	            .find(|t| t.first().map(|s| s == "expiration").unwrap_or(false))
    69	            .and_then(|t| t.get(1))
    70	            .and_then(|s| s.parse::<u64>().ok())
    71	    }
    72	
    73	    /// Returns all `e`-tag target ids (lowercase hex).
    74	    pub fn e_tags(&self) -> Vec<String> {
    75	        self.tags
    76	            .iter()
    77	            .filter(|t| t.first().map(|s| s == "e").unwrap_or(false))
    78	            .filter_map(|t| t.get(1).cloned())
    79	            .collect()
    80	    }
    81	
    82	    /// Returns all `p`-tag target pubkeys (lowercase hex).
    83	    pub fn p_tags(&self) -> Vec<String> {
    84	        self.tags
    85	            .iter()
    86	            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
    87	            .filter_map(|t| t.get(1).cloned())
    88	            .collect()
    89	    }
    90	
    91	    /// Returns all `a`-tag target addresses (e.g. "30023:pubkey:dtag").
    92	    pub fn a_tags(&self) -> Vec<String> {
    93	        self.tags
    94	            .iter()
    95	            .filter(|t| t.first().map(|s| s == "a").unwrap_or(false))
    96	            .filter_map(|t| t.get(1).cloned())
    97	            .collect()
    98	    }
    99	
   100	    /// Validates the event has a plausible structure (non-empty id, pubkey, sig).
   101	    /// Full cryptographic verification is deferred until the nostr crate is wired in.
   102	    pub fn is_structurally_valid(&self) -> bool {
   103	        self.id.len() == 64 && self.pubkey.len() == 64 && self.sig.len() == 128
   104	    }
   105	
   106	    /// Hex-decode this event's id. Used internally by mem/insert.rs.
   107	    pub(crate) fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
   108	        let mut out = [0u8; 32];
   109	        if s.len() != 64 {
   110	            return out;
   111	        }
   112	        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
   113	            if i >= 32 {
   114	                break;
   115	            }
   116	            if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
   117	                out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
   118	            }
   119	        }
   120	        out
   121	    }
   122	}
   123	
   124	// ─── VerifiedEvent ───────────────────────────────────────────────────────────
   125	
   126	/// A `RawEvent` that has passed cryptographic verification (id hash + Schnorr
   127	/// signature). This is the only type accepted by `EventStore::insert`.
   128	///
   129	/// Construction is intentionally limited to `try_from_raw()`. In tests and
   130	/// integration-test harnesses, `from_raw_unchecked()` bypasses verification
   131	/// (gated on `cfg(any(test, feature = "test-support"))`).
   132	pub struct VerifiedEvent(pub(crate) RawEvent);
   133	
   134	impl VerifiedEvent {
   135	    /// Verify `raw` and, if valid, wrap it in `VerifiedEvent`.
   136	    ///
   137	    /// Internally serializes `raw` to the NIP-01 canonical JSON, parses it
   138	    /// with the `nostr` crate, and calls `nostr::Event::verify()` which checks
   139	    /// both the event-id hash and the Schnorr signature.
   140	    pub fn try_from_raw(raw: RawEvent) -> Result<Self, VerifyError> {
   141	        use nostr::util::JsonUtil as _;
   142	        let json = serde_json::to_string(&raw)
   143	            .map_err(|e| VerifyError::Serialization(e.to_string()))?;
   144	        let ev = nostr::Event::from_json(&json)
   145	            .map_err(|_| VerifyError::InvalidId)?;
   146	        // verify() checks both event-id hash and Schnorr signature.
   147	        ev.verify().map_err(|e| {
   148	            let msg = e.to_string();
   149	            if msg.contains("id") {
   150	                VerifyError::InvalidId
   151	            } else {
   152	                VerifyError::InvalidSignature
   153	            }
   154	        })?;
   155	        Ok(VerifiedEvent(raw))
   156	    }
   157	
   158	    /// Bypass verification — only available in test and integration-test builds.
   159	    ///
   160	    /// Use this in store harnesses and unit tests where synthetic events with
   161	    /// placeholder signatures are needed. NEVER enabled in production builds.
   162	    #[cfg(any(test, feature = "test-support"))]
   163	    pub fn from_raw_unchecked(raw: RawEvent) -> Self {
   164	        VerifiedEvent(raw)
   165	    }
   166	
   167	    /// Access the underlying raw event.
   168	    pub fn raw(&self) -> &RawEvent {
   169	        &self.0
   170	    }
   171	
   172	    /// Consume and return the underlying raw event.
   173	    pub fn into_raw(self) -> RawEvent {
   174	        self.0
   175	    }
   176	}
   177	
   178	// ─── StoredEvent ─────────────────────────────────────────────────────────────
   179	
   180	/// A stored Nostr event with arrival metadata.
   181	///
   182	/// `raw` is `Arc<RawEvent>` so the hot LRU can hold reference-counted copies
   183	/// without cloning the event body on each `get_by_id`.
   184	#[derive(Clone, Debug)]
   185	pub struct StoredEvent {
   186	    pub raw: Arc<RawEvent>,
   187	    pub received_at_ms: u64,   // wall-clock first arrival across all relays
   188	}

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/store/mem/gc.rs | sed -n '1,130p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! Claim / release / gc_step for `MemEventStore`.
     2	//!
     3	//! Implements the HotSet semantics from `docs/design/lmdb/gc.md` §2:
     4	//!   - per-view ceiling: `DEFAULT_VIEW_CEILING` (1000 events).
     5	//!   - global pinned ceiling: `MAX_PINNED_TOTAL` (20000 events).
     6	//!   - BTreeSet idempotency per T25: re-claiming a known id is a no-op.
     7	//!   - `StoreError::OverPinned` on breach (D8).
     8	
     9	use super::{bytes_to_hex, MemEventStore, DEFAULT_VIEW_CEILING, MAX_PINNED_TOTAL, TOMBSTONE_MAX_AGE_SECS};
    10	use crate::store::types::{
    11	    ClaimerId, EventId, GcBudget, GcReport, TombstoneOrigin, TombstoneRow,
    12	};
    13	use crate::store::StoreError;
    14	
    15	pub(super) fn register_view_cover(
    16	    store: &MemEventStore,
    17	    claimer: ClaimerId,
    18	    cover_budget: usize,
    19	) -> Result<(), StoreError> {
    20	    let mut st = store.lock()?;
    21	    st.claim_budgets.insert(claimer, cover_budget);
    22	    Ok(())
    23	}
    24	
    25	pub(super) fn claim(
    26	    store: &MemEventStore,
    27	    claimer: ClaimerId,
    28	    ids: &[EventId],
    29	) -> Result<(), StoreError> {
    30	    let mut st = store.lock()?;
    31	    let ceiling = *st.claim_budgets.get(&claimer).unwrap_or(&DEFAULT_VIEW_CEILING);
    32	
    33	    let existing_set = st.claims.entry(claimer).or_default();
    34	    // Only count genuinely new ids (BTreeSet idempotency).
    35	    let new_ids: Vec<String> = ids
    36	        .iter()
    37	        .map(|id| bytes_to_hex(id))
    38	        .filter(|hex| !existing_set.contains(hex))
    39	        .collect();
    40	
    41	    let current_for_claimer = existing_set.len();
    42	    let requested_for_claimer = current_for_claimer + new_ids.len();
    43	    if requested_for_claimer > ceiling {
    44	        return Err(StoreError::OverPinned {
    45	            claimer,
    46	            requested: requested_for_claimer,
    47	            ceiling,
    48	        });
    49	    }
    50	
    51	    // Global pinned ceiling check (D8 / gc.md §2).
    52	    let all_pinned: usize = st.claims.values().map(|s| s.len()).sum();
    53	    let global_new = new_ids
    54	        .iter()
    55	        .filter(|hex| !st.claims.values().any(|s| s.contains(*hex)))
    56	        .count();
    57	    let requested_global = all_pinned + global_new;
    58	    if requested_global > MAX_PINNED_TOTAL {
    59	        return Err(StoreError::OverPinned {
    60	            claimer,
    61	            requested: requested_global,
    62	            ceiling: MAX_PINNED_TOTAL,
    63	        });
    64	    }
    65	
    66	    // Apply the claims.
    67	    let set = st.claims.entry(claimer).or_default();
    68	    for hex in new_ids {
    69	        set.insert(hex);
    70	    }
    71	    Ok(())
    72	}
    73	
    74	pub(super) fn release(
    75	    store: &MemEventStore,
    76	    claimer: ClaimerId,
    77	) -> Result<(), StoreError> {
    78	    let mut st = store.lock()?;
    79	    st.claims.remove(&claimer);
    80	    // Leave budget registered — re-registering at re-open is the actor's job.
    81	    Ok(())
    82	}
    83	
    84	pub(super) fn gc_step(
    85	    store: &MemEventStore,
    86	    budget: GcBudget,
    87	) -> Result<GcReport, StoreError> {
    88	    let start = std::time::Instant::now();
    89	    let mut st = store.lock()?;
    90	    let mut report = GcReport::default();
    91	
    92	    let now_ms = std::time::SystemTime::now()
    93	        .duration_since(std::time::UNIX_EPOCH)
    94	        .map(|d| d.as_millis() as u64)
    95	        .unwrap_or(0);
    96	    let now_secs = now_ms / 1000;
    97	
    98	    // Reap NIP-40 expired events.
    99	    let expired_ids: Vec<String> = st
   100	        .events
   101	        .iter()
   102	        .filter(|(_, ev)| ev.raw.expiration().is_some_and(|exp| exp <= now_secs))
   103	        .map(|(id, _)| id.clone())
   104	        .take(budget.max_events_per_step)
   105	        .collect();
   106	
   107	    for id_hex in &expired_ids {
   108	        if let Some(ev) = st.events.remove(id_hex) {
   109	            st.provenance.remove(id_hex);
   110	            st.tombstones.insert(
   111	                id_hex.clone(),
   112	                TombstoneRow {
   113	                    target_id: ev.raw.id_bytes(),
   114	                    kind5_event_id: None,
   115	                    deleter_pubkey: None,
   116	                    deleted_at: now_secs,
   117	                    sources: vec![],
   118	                    origin: TombstoneOrigin::NIP40Expiry,
   119	                },
   120	            );
   121	            report.expired_reaped += 1;
   122	        }
   123	        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
   124	            break;
   125	        }
   126	    }
   127	
   128	    // Purge tombstones older than TOMBSTONE_MAX_AGE_SECS.
   129	    let stale_tombstones: Vec<String> = st
   130	        .tombstones

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/store/mem/insert.rs | sed -n '1,380p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! §7.1 insert invariants for `MemEventStore`.
     2	//!
     3	//! D4: ONE writer. All event mutations flow through here.
     4	//! D2: Returns typed `InsertOutcome`; never panics.
     5	//!
     6	//! P2 fixes applied here:
     7	//!   - Duplicate check BEFORE kind-specific supersession (provenance merge).
     8	//!   - Tombstone max-merge (`deleted_at` max + source union instead of or_insert).
     9	
    10	use std::collections::HashMap;
    11	use std::sync::Arc;
    12	
    13	use super::{bytes_to_hex, upsert_provenance, MemEventStore, MemState};
    14	use crate::store::types::{
    15	    DeleteFilter, InsertOutcome, RawEvent, RejectReason, RelayUrl, StoredEvent,
    16	    TombstoneOrigin, TombstoneRow,
    17	};
    18	use crate::store::StoreError;
    19	
    20	// ─── Public entry points ─────────────────────────────────────────────────────
    21	
    22	pub(super) fn insert(
    23	    store: &MemEventStore,
    24	    event: RawEvent,
    25	    source: &RelayUrl,
    26	    received_at_ms: u64,
    27	) -> Result<InsertOutcome, StoreError> {
    28	    // 1. Structural validation (sig check deferred to nostr crate wiring).
    29	    if !event.is_structurally_valid() {
    30	        return Ok(InsertOutcome::Rejected {
    31	            id: event.id_bytes(),
    32	            reason: RejectReason::Malformed("invalid id/pubkey/sig length".into()),
    33	        });
    34	    }
    35	
    36	    // 2. Ephemeral: deliver to live consumers, do not store.
    37	    if event.is_ephemeral() {
    38	        return Ok(InsertOutcome::Ephemeral { id: event.id_bytes() });
    39	    }
    40	
    41	    // 3. Check NIP-40 expiration on arrival.
    42	    if let Some(exp) = event.expiration() {
    43	        let now_secs = received_at_ms / 1000;
    44	        if exp <= now_secs {
    45	            return Ok(InsertOutcome::Rejected {
    46	                id: event.id_bytes(),
    47	                reason: RejectReason::ExpiredOnArrival,
    48	            });
    49	        }
    50	    }
    51	
    52	    let id_bytes = event.id_bytes();
    53	    let id_hex = event.id.clone();
    54	    let mut st = store.lock()?;
    55	
    56	    // 4. Check per-id tombstone.
    57	    // Foreign kind:5 pre-tombstones (deleter != author) must NOT block the event.
    58	    if let Some(tomb) = st.tombstones.get(&id_hex).cloned() {
    59	        let applies = match tomb.origin {
    60	            TombstoneOrigin::Kind5 => tomb
    61	                .deleter_pubkey
    62	                .as_ref()
    63	                .map(|dp| bytes_to_hex(dp) == event.pubkey)
    64	                .unwrap_or(false),
    65	            TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
    66	        };
    67	        if applies {
    68	            return Ok(InsertOutcome::Tombstoned {
    69	                id: id_bytes,
    70	                kind5_event_id: tomb.kind5_event_id,
    71	                origin: tomb.origin,
    72	            });
    73	        }
    74	        // Foreign pre-tombstone — remove and allow insert (invariant 3).
    75	        st.tombstones.remove(&id_hex);
    76	    }
    77	
    78	    // 5. Check address tombstone for parameterized replaceables.
    79	    if event.is_param_replaceable() {
    80	        if let Some(d) = event.d_tag() {
    81	            let addr_key = format!(
    82	                "{}:{}:{}",
    83	                event.kind,
    84	                event.pubkey,
    85	                String::from_utf8_lossy(&d)
    86	            );
    87	            if let Some(tomb) = st.addr_tombstones.get(&addr_key) {
    88	                if tomb.deleted_at >= event.created_at {
    89	                    return Ok(InsertOutcome::Tombstoned {
    90	                        id: id_bytes,
    91	                        kind5_event_id: tomb.kind5_event_id,
    92	                        origin: tomb.origin,
    93	                    });
    94	                }
    95	            }
    96	        }
    97	    }
    98	
    99	    // 6. Kind:5 self-delete handling.
   100	    if event.kind == 5 {
   101	        return handle_kind5_insert(&mut st, event, source, received_at_ms);
   102	    }
   103	
   104	    // 7. Replaceable supersession.
   105	    if event.is_replaceable() {
   106	        let key = (event.pubkey.clone(), event.kind, None::<String>);
   107	        return handle_supersession(&mut st, event, source, received_at_ms, key);
   108	    }
   109	
   110	    // 8. Parameterized replaceable.
   111	    if event.is_param_replaceable() {
   112	        let d = event.d_tag()
   113	            .map(|b| String::from_utf8_lossy(&b).into_owned());
   114	        let key = (event.pubkey.clone(), event.kind, d);
   115	        return handle_supersession(&mut st, event, source, received_at_ms, key);
   116	    }
   117	
   118	    // 9. Normal insert / duplicate.
   119	    handle_normal_insert(&mut st, event, source, received_at_ms)
   120	}
   121	
   122	pub(super) fn delete_by_filter(
   123	    store: &MemEventStore,
   124	    filter: DeleteFilter,
   125	) -> Result<usize, StoreError> {
   126	    let mut st = store.lock()?;
   127	    let ids_to_remove: Vec<String> = match &filter {
   128	        DeleteFilter::ByRelayOnly(relay) => st
   129	            .events
   130	            .keys()
   131	            .filter(|id| {
   132	                st.provenance
   133	                    .get(*id)
   134	                    .map(|p| p.len() == 1 && p[0].relay_url == *relay)
   135	                    .unwrap_or(false)
   136	            })
   137	            .cloned()
   138	            .collect(),
   139	        DeleteFilter::ByAuthor(pk) => {
   140	            let pk_hex = bytes_to_hex(pk);
   141	            st.events
   142	                .iter()
   143	                .filter(|(_, ev)| ev.raw.pubkey == pk_hex)
   144	                .map(|(id, _)| id.clone())
   145	                .collect()
   146	        }
   147	        DeleteFilter::ByIds(ids) => ids
   148	            .iter()
   149	            .map(|id| bytes_to_hex(id))
   150	            .filter(|h| st.events.contains_key(h))
   151	            .collect(),
   152	        DeleteFilter::ByKindRange { lo, hi } => st
   153	            .events
   154	            .iter()
   155	            .filter(|(_, ev)| ev.raw.kind >= *lo && ev.raw.kind <= *hi)
   156	            .map(|(id, _)| id.clone())
   157	            .collect(),
   158	    };
   159	    let count = ids_to_remove.len();
   160	    for id in ids_to_remove {
   161	        st.events.remove(&id);
   162	        st.provenance.remove(&id);
   163	    }
   164	    Ok(count)
   165	}
   166	
   167	// ─── Shared supersession helper ───────────────────────────────────────────────
   168	
   169	/// Unified supersession logic for both replaceable and param-replaceable kinds.
   170	/// `key` = (pubkey_hex, kind, Option<d_tag_str>) — None means any d-tag (replaceable).
   171	fn handle_supersession(
   172	    st: &mut MemState,
   173	    event: RawEvent,
   174	    source: &RelayUrl,
   175	    received_at_ms: u64,
   176	    key: (String, u32, Option<String>),
   177	) -> Result<InsertOutcome, StoreError> {
   178	    let id_bytes = event.id_bytes();
   179	    let id_hex = event.id.clone();
   180	    let (pubkey_hex, kind, d_tag_filter) = key;
   181	
   182	    // P2 fix: exact-id duplicate BEFORE supersession check.
   183	    if st.events.contains_key(&id_hex) {
   184	        let p = st.provenance.entry(id_hex).or_default();
   185	        upsert_provenance(p, source.clone(), received_at_ms);
   186	        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after: p.len() as u32 });
   187	    }
   188	
   189	    let existing_id: Option<String> = st
   190	        .events
   191	        .iter()
   192	        .filter(|(_, ev)| {
   193	            ev.raw.pubkey == pubkey_hex
   194	                && ev.raw.kind == kind
   195	                && match &d_tag_filter {
   196	                    None => true,
   197	                    Some(d) => ev.raw.d_tag()
   198	                        .map(|tag| String::from_utf8_lossy(&tag).into_owned() == *d)
   199	                        .unwrap_or(false),
   200	                }
   201	        })
   202	        .max_by(|(_, a), (_, b)| {
   203	            a.raw.created_at
   204	                .cmp(&b.raw.created_at)
   205	                .then(b.raw.id.cmp(&a.raw.id))
   206	        })
   207	        .map(|(id, _)| id.clone());
   208	
   209	    if let Some(ref existing_hex) = existing_id {
   210	        let existing_ev = &st.events[existing_hex];
   211	        let existing_time = existing_ev.raw.created_at;
   212	        let existing_id_str = existing_ev.raw.id.clone();
   213	        let incoming_wins = event.created_at > existing_time
   214	            || (event.created_at == existing_time && event.id < existing_id_str);
   215	
   216	        if incoming_wins {
   217	            let replaced_id = hex_to_bytes32_owned(existing_hex);
   218	            st.events.remove(existing_hex);
   219	            st.provenance.remove(existing_hex);
   220	            let new_id = id_bytes;
   221	            st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
   222	            let p = st.provenance.entry(id_hex).or_default();
   223	            upsert_provenance(p, source.clone(), received_at_ms);
   224	            Ok(InsertOutcome::Replaced { new_id, replaced_id })
   225	        } else {
   226	            Ok(InsertOutcome::Superseded { id: id_bytes, current_id: hex_to_bytes32_owned(existing_hex) })
   227	        }
   228	    } else {
   229	        st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
   230	        let p = st.provenance.entry(id_hex).or_default();
   231	        upsert_provenance(p, source.clone(), received_at_ms);
   232	        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after: p.len() as u32 })
   233	    }
   234	}
   235	
   236	fn handle_normal_insert(
   237	    st: &mut MemState,
   238	    event: RawEvent,
   239	    source: &RelayUrl,
   240	    received_at_ms: u64,
   241	) -> Result<InsertOutcome, StoreError> {
   242	    let id_bytes = event.id_bytes();
   243	    let id_hex = event.id.clone();
   244	
   245	    if st.events.contains_key(&id_hex) {
   246	        let p = st.provenance.entry(id_hex.clone()).or_default();
   247	        upsert_provenance(p, source.clone(), received_at_ms);
   248	        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after: p.len() as u32 });
   249	    }
   250	
   251	    st.events.insert(id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
   252	    let p = st.provenance.entry(id_hex).or_default();
   253	    upsert_provenance(p, source.clone(), received_at_ms);
   254	    Ok(InsertOutcome::Inserted { id: id_bytes, sources_after: p.len() as u32 })
   255	}
   256	
   257	fn handle_kind5_insert(
   258	    st: &mut MemState,
   259	    event: RawEvent,
   260	    source: &RelayUrl,
   261	    received_at_ms: u64,
   262	) -> Result<InsertOutcome, StoreError> {
   263	    let kind5_id_bytes = event.id_bytes();
   264	    let kind5_id_hex = event.id.clone();
   265	    let kind5_pubkey = event.pubkey.clone();
   266	    let kind5_at = event.created_at;
   267	
   268	    // Process `e`-tag deletes (self-deletes only).
   269	    for target_hex in event.e_tags() {
   270	        if let Some(existing) = st.events.get(&target_hex) {
   271	            if existing.raw.pubkey != kind5_pubkey { continue; }
   272	            let target_id = existing.raw.id_bytes();
   273	            st.events.remove(&target_hex);
   274	            st.provenance.remove(&target_hex);
   275	            merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source));
   276	        } else {
   277	            let target_id = hex_to_bytes32_owned(&target_hex);
   278	            merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(target_id, kind5_id_bytes, &kind5_pubkey, kind5_at, source));
   279	        }
   280	    }
   281	
   282	    // Process `a`-tag deletes (parameterized replaceables, self-delete only).
   283	    for addr in event.a_tags() {
   284	        let parts: Vec<&str> = addr.splitn(3, ':').collect();
   285	        if parts.len() < 3 { continue; }
   286	        let (tgt_kind_str, tgt_pk, tgt_dtag) = (parts[0], parts[1], parts[2]);
   287	        if tgt_pk != kind5_pubkey { continue; }
   288	        let Ok(tgt_kind) = tgt_kind_str.parse::<u32>() else { continue };
   289	        let addr_key = format!("{}:{}:{}", tgt_kind_str, tgt_pk, tgt_dtag);
   290	
   291	        let to_delete: Vec<String> = st.events.iter()
   292	            .filter(|(_, ev)| {
   293	                ev.raw.pubkey == tgt_pk && ev.raw.kind == tgt_kind
   294	                    && ev.raw.d_tag().map(|d| String::from_utf8_lossy(&d).into_owned() == tgt_dtag).unwrap_or(false)
   295	                    && ev.raw.created_at <= kind5_at
   296	            })
   297	            .map(|(id, _)| id.clone())
   298	            .collect();
   299	
   300	        for target_hex in to_delete {
   301	            if let Some(existing) = st.events.remove(&target_hex) {
   302	                st.provenance.remove(&target_hex);
   303	                merge_tombstone(&mut st.tombstones, target_hex, kind5_tomb(existing.raw.id_bytes(), kind5_id_bytes, &kind5_pubkey, kind5_at, source));
   304	            }
   305	        }
   306	        // Address tombstone for events arriving later (max-merge).
   307	        merge_tombstone(&mut st.addr_tombstones, addr_key, kind5_tomb([0u8; 32], kind5_id_bytes, &kind5_pubkey, kind5_at, source));
   308	    }
   309	
   310	    // Store the kind:5 event itself.
   311	    st.events.insert(kind5_id_hex.clone(), StoredEvent { raw: Arc::new(event), received_at_ms });
   312	    let p = st.provenance.entry(kind5_id_hex).or_default();
   313	    upsert_provenance(p, source.clone(), received_at_ms);
   314	    Ok(InsertOutcome::Inserted { id: kind5_id_bytes, sources_after: p.len() as u32 })
   315	}
   316	
   317	// ─── Tombstone helpers ────────────────────────────────────────────────────────
   318	
   319	fn kind5_tomb(
   320	    target_id: [u8; 32],
   321	    kind5_id: [u8; 32],
   322	    kind5_pubkey: &str,
   323	    deleted_at: u64,
   324	    source: &RelayUrl,
   325	) -> TombstoneRow {
   326	    TombstoneRow {
   327	        target_id,
   328	        kind5_event_id: Some(kind5_id),
   329	        deleter_pubkey: Some(hex_to_bytes32_owned(kind5_pubkey)),
   330	        deleted_at,
   331	        sources: vec![source.clone()],
   332	        origin: TombstoneOrigin::Kind5,
   333	    }
   334	}
   335	
   336	/// P2 fix: tombstone upsert max-merges `deleted_at` and unions sources.
   337	/// Original `or_insert` kept first-arrived timestamp — wrong for re-deliveries.
   338	fn merge_tombstone(map: &mut HashMap<String, TombstoneRow>, key: String, incoming: TombstoneRow) {
   339	    match map.get_mut(&key) {
   340	        Some(existing) => {
   341	            if incoming.deleted_at > existing.deleted_at {
   342	                existing.deleted_at = incoming.deleted_at;
   343	                existing.kind5_event_id = incoming.kind5_event_id;
   344	            }
   345	            for src in incoming.sources {
   346	                if !existing.sources.contains(&src) {
   347	                    existing.sources.push(src);
   348	                }
   349	            }
   350	        }
   351	        None => { map.insert(key, incoming); }
   352	    }
   353	}
   354	
   355	fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
   356	    RawEvent::hex_to_bytes32_owned(s)
   357	}

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/store/mem/gc.rs | sed -n '128,230p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   128	    // Purge tombstones older than TOMBSTONE_MAX_AGE_SECS.
   129	    let stale_tombstones: Vec<String> = st
   130	        .tombstones
   131	        .iter()
   132	        .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
   133	        .map(|(k, _)| k.clone())
   134	        .collect();
   135	    report.tombstones_purged = stale_tombstones.len();
   136	    for k in stale_tombstones {
   137	        st.tombstones.remove(&k);
   138	    }
   139	
   140	    report.duration_ms = start.elapsed().as_millis() as u32;
   141	    Ok(report)
   142	}
   143	
   144	// ─── Tests ───────────────────────────────────────────────────────────────────
   145	
   146	#[cfg(test)]
   147	mod tests {
   148	    use super::*;
   149	    use crate::store::{EventStore, MemEventStore};
   150	    use crate::store::types::EventId;
   151	
   152	    fn make_id(b: u8) -> EventId {
   153	        let mut id = [0u8; 32];
   154	        id[0] = b;
   155	        id
   156	    }
   157	
   158	    #[test]
   159	    fn claim_idempotent_reclaim_does_not_count() {
   160	        let store = MemEventStore::new();
   161	        let c = ClaimerId(1);
   162	        store.register_view_cover(c, 5).unwrap();
   163	        let id = make_id(1);
   164	        store.claim(c, &[id]).unwrap();
   165	        // Re-claiming the same id must not count toward the ceiling.
   166	        store.claim(c, &[id]).unwrap();
   167	        let st = store.lock().unwrap();
   168	        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
   169	    }
   170	
   171	    #[test]
   172	    fn claim_over_per_view_ceiling_returns_err() {
   173	        let store = MemEventStore::new();
   174	        let c = ClaimerId(2);
   175	        store.register_view_cover(c, 2).unwrap();
   176	        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
   177	        let result = store.claim(c, &[make_id(3)]);
   178	        assert!(
   179	            matches!(result, Err(StoreError::OverPinned { .. })),
   180	            "must return OverPinned when per-view ceiling exceeded"
   181	        );
   182	    }
   183	
   184	    #[test]
   185	    fn release_clears_all_pins() {
   186	        let store = MemEventStore::new();
   187	        let c = ClaimerId(3);
   188	        store.register_view_cover(c, 100).unwrap();
   189	        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
   190	        store.release(c).unwrap();
   191	        let st = store.lock().unwrap();
   192	        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
   193	    }
   194	}

exec
/bin/zsh -lc "nl -ba docs/design/lmdb/gc.md | sed -n '1,220p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: GC working-set policy
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
     4	
     5	## 1. Definitions
     6	
     7	```
     8	stored_events = every event currently in `events` (primary), not tombstoned
     9	
    10	claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
    11	                where each `claimer` is an open ViewHandle / open ActionHandle
    12	
    13	open_view_cover = ⋃ { dependency_target_ids(spec)
    14	                       | (view_id, spec) ∈ open_views }
    15	                  computed from the composite reverse-index per ADR-0001
    16	
    17	recently_touched = top-N by `last_touched_ms` (default N = 10,000)
    18	
    19	hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
    20	cold         = stored_events \ hot_resident
    21	```
    22	
    23	`last_touched_ms` is bumped on every `get_by_id`, on every secondary scan that *materialises* the event body, and on `insert` for a fresh row. Scans that only return ids/timestamps (e.g., the early-filter pass in a view's planner) do **not** bump it — only the construction of a `Delta` payload that needs the body does.
    24	
    25	`hot_resident` is stored in memory; `cold` lives only on disk. The store still **knows** about every cold event via secondaries — the reverse index covers both per ADR-0003: "The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend."
    26	
    27	## 2. Hot data structure
    28	
    29	```rust
    30	pub(crate) struct HotSet {
    31	    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
    32	    lru: lru::LruCache<EventId, Arc<nostr::Event>>,
    33	    // Strong-pin overlay; refcounted by ClaimerId.
    34	    pinned: HashMap<EventId, u32>,                   // event_id → refcount
    35	    // Reverse map for cheap release(); BTreeSet ensures claim() is idempotent per claimer.
    36	    by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>,
    37	    // Per-view ceiling registered by register_view_cover().
    38	    view_budgets: HashMap<ClaimerId, usize>,
    39	    target_hot_size: usize,
    40	    // Ceilings (enforced on every claim() call — D8 / ADR-0001..0004).
    41	    max_claim_per_view: usize,   // default 1_000; callers may lower via register_view_cover
    42	    max_pinned_total: usize,     // default 20_000; hard cap on pinned.len()
    43	}
    44	
    45	impl HotSet {
    46	    /// Record the budget for a view before its first claim. If not called, the
    47	    /// default `max_claim_per_view` applies. Calling it again with a lower budget
    48	    /// after claims have already been issued does *not* retroactively reject them;
    49	    /// the lower ceiling applies to future claim() calls.
    50	    pub fn register_view_cover(&mut self, c: ClaimerId, budget: usize) {
    51	        self.view_budgets.insert(c, budget);
    52	    }
    53	
    54	    /// Pin `ids` for `c`. Idempotent: re-claiming an id already in the claimer's set
    55	    /// is a no-op (refcount not double-incremented). Budget checks count only genuinely
    56	    /// new ids. Returns `StoreError::OverPinned` if limits would be exceeded.
    57	    /// On rejection, the state is unchanged (all-or-nothing per call).
    58	    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
    59	        let existing = self.by_claimer.get(&c);
    60	        // Collect into a BTreeSet to dedup both intra-call and against already-claimed ids.
    61	        let new_ids: BTreeSet<EventId> = ids.iter()
    62	            .filter(|id| existing.map_or(true, |s| !s.contains(*id)))
    63	            .copied()
    64	            .collect();
    65	        let per_view_ceiling = self.view_budgets
    66	            .get(&c)
    67	            .copied()
    68	            .unwrap_or(self.max_claim_per_view);
    69	        let current_for_claimer = existing.map_or(0, |s| s.len());
    70	        if current_for_claimer + new_ids.len() > per_view_ceiling {
    71	            return Err(StoreError::OverPinned {
    72	                claimer: c,
    73	                requested: current_for_claimer + new_ids.len(),
    74	                ceiling: per_view_ceiling,
    75	            });
    76	        }
    77	        let new_global = self.pinned.len() + new_ids.iter()
    78	            .filter(|id| !self.pinned.contains_key(id))
    79	            .count();
    80	        if new_global > self.max_pinned_total {
    81	            return Err(StoreError::OverPinned {
    82	                claimer: c,
    83	                requested: new_global,
    84	                ceiling: self.max_pinned_total,
    85	            });
    86	        }
    87	        let set = self.by_claimer.entry(c).or_default();
    88	        for id in &new_ids {
    89	            set.insert(*id);
    90	            *self.pinned.entry(*id).or_insert(0) += 1;
    91	        }
    92	        Ok(())
    93	    }
    94	
    95	    pub fn release(&mut self, c: ClaimerId) {
    96	        if let Some(ids) = self.by_claimer.remove(&c) {
    97	            for id in ids {
    98	                if let Some(rc) = self.pinned.get_mut(&id) {
    99	                    *rc = rc.saturating_sub(1);
   100	                    if *rc == 0 { self.pinned.remove(&id); }
   101	                }
   102	            }
   103	        }
   104	        self.view_budgets.remove(&c);
   105	    }
   106	
   107	    pub fn touch(&mut self, id: EventId, e: Arc<nostr::Event>) {
   108	        self.lru.put(id, e);                          // bumps LRU
   109	        self.trim();
   110	    }
   111	
   112	    fn trim(&mut self) {
   113	        while self.lru.len() > self.target_hot_size {
   114	            // pop_lru returns oldest; skip pinned ones until we find an evictable.
   115	            // (LruCache::pop_lru doesn't take a predicate; we rotate.)
   116	            let mut skipped: SmallVec<[(EventId, Arc<nostr::Event>); 8]> = SmallVec::new();
   117	            let evicted = loop {
   118	                match self.lru.pop_lru() {
   119	                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
   120	                    Some(pair) => break Some(pair),
   121	                    None => break None,
   122	                }
   123	            };
   124	            for (id, e) in skipped.drain(..) { self.lru.put(id, e); }
   125	            // If every LRU entry is pinned, the overflow will not be resolved by
   126	            // trim() alone. The working-set budget enforcement in claim() is the
   127	            // primary defence; trim() stopping here is intentional, not a silent
   128	            // acceptance of unbounded growth.
   129	            if evicted.is_none() { break; }
   130	        }
   131	    }
   132	}
   133	```
   134	
   135	`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).
   136	
   137	**Ceiling defaults** (see `StoreError::OverPinned` in [`trait/types.md`](trait/types.md)):
   138	- `max_claim_per_view`: 1 000 events per claimer. A view that tries to pin more returns `OverPinned`; the actor surfaces this as `Effect::ViewOverPinned` and releases the claim.
   139	- `max_pinned_total`: 20 000 events globally. Prevents many moderate-sized views from collectively overwhelming the working set (D8 / ADR-0003 gate).
   140	
   141	These defaults allow 100 active views × 200 pins each = 20 000 globally, within the ADR-0003 §5 memory accounting (10k LRU + 20k pinned overlay ≈ 90 MB, under the 100 MB gate).
   142	
   143	## 3. `gc_step()` algorithm
   144	
   145	```rust
   146	pub fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
   147	    let start = Instant::now();
   148	    let now_s = unix_now();
   149	    let mut report = GcReport::default();
   150	
   151	    // 3.1 — NIP-40 expired reaper.
   152	    let to_reap = self.scan_expiring_before(now_s, budget.max_events_per_step)?
   153	        .collect::<Result<Vec<_>, _>>()?;
   154	    for ev in to_reap {
   155	        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms { break; }
   156	        self.reap_one(ev.raw.id.into(), TombstoneOrigin::NIP40Expiry, now_s)?;
   157	        report.expired_reaped += 1;
   158	    }
   159	
   160	    // 3.2 — Trim LRU back to target.
   161	    let lru_before = self.hot.lock().lru.len();
   162	    self.hot.lock().trim();
   163	    report.lru_evicted = lru_before.saturating_sub(self.hot.lock().lru.len());
   164	
   165	    // 3.3 — Purge old tombstones whose target event is absent.
   166	    let cutoff = now_s.saturating_sub(self.cfg.tombstone_retention_secs);
   167	    report.tombstones_purged = self.purge_old_tombstones(cutoff,
   168	        budget.max_events_per_step.saturating_sub(report.expired_reaped))?;
   169	
   170	    report.duration_ms = start.elapsed().as_millis() as u32;
   171	    Ok(report)
   172	}
   173	```
   174	
   175	Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms }`. Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`. The actor calls `gc_step()`:
   176	
   177	- Every 60 seconds (cooperative; runs on the actor thread between mailbox messages).
   178	- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals).
   179	- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net).
   180	
   181	`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform.
   182	
   183	## 4. Claim / release wiring
   184	
   185	The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:
   186	
   187	1. The view module's `dependencies(spec)` is consulted (per `kernel-substrate.md` §3).
   188	2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
   189	3. `store.register_view_cover(claimer_id, cover_budget)` registers the budget ceiling for this view. `cover_budget` is `spec.max_cover_size()` (a per-view-module constant; defaults to 200 if unspecified).
   190	4. `store.claim(claimer_id, &cover_ids)` pins those events in hot. Returns `StoreError::OverPinned` if the registered budget is exceeded; the actor releases the claim and surfaces `Effect::ViewOverPinned`.
   191	5. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally. Because `by_claimer` uses `BTreeSet<EventId>`, re-claiming an already-pinned id is a no-op — the refcount in `pinned` is not double-incremented.
   192	
   193	On `close_view(view_id)`:
   194	
   195	1. `store.release(claimer_id)` drops every pin in one call.
   196	2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.
   197	
   198	Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.
   199	
   200	## 5. Memory accounting (the ADR-0003 gate)
   201	
   202	The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
   203	
   204	Components measured:
   205	
   206	| Source | Approx bytes | Notes |
   207	|---|---|---|
   208	| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
   209	| Claim refcount maps (≤20k pinned + 10k LRU entries) | ~1 MB | `HashMap<EventId, u32>` + reverse `by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>` + `view_budgets`; global ceiling 20k pins keeps this bounded |
   210	| Reverse index in-memory (composite keys for 100 views) | ~5 MB | from ADR-0001 — bounded by `~broad_axes_guardrail` per ADR-0001 |
   211	| Projection caches (author display, reaction counts) | ~10 MB | LRU-bounded by referenced-view count per ADR-0003 |
   212	| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
   213	| Watermarks (loaded as `HashMap` for hot lookups) | ~2 MB | M4 — assuming O(10k) watermarks (one per `(filter, relay)` pair) |
   214	| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
   215	| Action ledger in-flight rows | ~1 MB | bounded by spec §7.5 |
   216	| Slack / Rust allocator overhead | ~20 MB | empirical from reactivity-bench |
   217	| **Total target** | **~70 MB** | leaves ~30 MB headroom against the 100 MB gate |
   218	
   219	The 1M-events-on-disk dimension does **not** appear in the budget because LMDB does not page them into our heap; they exist in mmap'd pages the OS may evict at will. This is the design intent of ADR-0003.
   220	

exec
/bin/zsh -lc 'rg -n "global|20_000|20000|pinned|claim|HotSet|D8" docs/design/lmdb docs/product-spec/overview-and-dx.md crates/nmp-core/src/store -S' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/product-spec/overview-and-dx.md:27:## 1.5 Cardinal doctrines D0–D8
docs/product-spec/overview-and-dx.md:31:Summary: D0–D5 are *policy* doctrines (user-facing semantics); D6–D8 are *substrate invariants* (runtime implementation constraints). Both are equally binding. Every API decision answers to at least one; conflicts resolve in the order D0 > D1 > … > D8.
docs/product-spec/overview-and-dx.md:43:| D8 | Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded; idle-tick emit gated on `changed_since_emit()` | Substrate |
docs/product-spec/overview-and-dx.md:75:Acceptance is **demonstrable, not aspirational**. A claim that the framework works is provable by running these:
docs/product-spec/overview-and-dx.md:191:- Wallet operations: NWC + Cashu + zaps in both directions + nutzap claim.
docs/design/lmdb/watermarks.md:191:- `StoreHealth.watermark_count` (per [`gc.md`](gc.md) §7) summarises the global count.
crates/nmp-core/src/store/mem/store_impl.rs:138:        claimer: ClaimerId,
crates/nmp-core/src/store/mem/store_impl.rs:141:        gc::register_view_cover(self, claimer, cover_budget)
crates/nmp-core/src/store/mem/store_impl.rs:144:    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
crates/nmp-core/src/store/mem/store_impl.rs:145:        gc::claim(self, claimer, ids)
crates/nmp-core/src/store/mem/store_impl.rs:148:    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
crates/nmp-core/src/store/mem/store_impl.rs:149:        gc::release(self, claimer)
docs/design/lmdb/trait.md:42:    /// Used for global parameterized-replaceable discovery (e.g. "recent articles with slug X").
docs/design/lmdb/trait.md:128:    // ─────── Hot-set / claims (GC) ───────
docs/design/lmdb/trait.md:131:    /// Must be called before `claim()` for a given `claimer`. If not called,
docs/design/lmdb/trait.md:132:    /// the store applies a default per-view ceiling of `max_claim_per_view` events
docs/design/lmdb/trait.md:135:    /// Enforcement: `claim()` counts the current per-claimer set size; if adding
docs/design/lmdb/trait.md:136:    /// `ids` would exceed this budget OR the global `max_pinned_total` ceiling,
docs/design/lmdb/trait.md:138:    /// The caller is responsible for releasing stale claims first.
docs/design/lmdb/trait.md:140:    /// Rationale: D8 (reactivity contract) requires that the kernel's working-set
docs/design/lmdb/trait.md:143:    fn register_view_cover(&self, claimer: ClaimerId, cover_budget: usize) -> Result<(), StoreError>;
docs/design/lmdb/trait.md:146:    /// if adding `ids` would exceed the per-claimer budget or the global ceiling.
docs/design/lmdb/trait.md:147:    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
docs/design/lmdb/trait.md:148:    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
docs/design/lmdb/trait.md:213:- `OverPinned` → the caller (actor) surfaces this as `Effect::ViewOverPinned { claimer }` and then calls `release(claimer)` to drop the offending claim, keeping the working set bounded per D8.
crates/nmp-core/src/store/mem/tests.rs:98:    fn claim_idempotent_reclaim_does_not_count() {
crates/nmp-core/src/store/mem/tests.rs:103:        store.claim(c, &[id]).unwrap();
crates/nmp-core/src/store/mem/tests.rs:104:        store.claim(c, &[id]).unwrap();
crates/nmp-core/src/store/mem/tests.rs:106:        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
crates/nmp-core/src/store/mem/tests.rs:110:    fn claim_over_per_view_ceiling_returns_err() {
crates/nmp-core/src/store/mem/tests.rs:114:        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
crates/nmp-core/src/store/mem/tests.rs:115:        let result = store.claim(c, &[make_id(3)]);
crates/nmp-core/src/store/mem/tests.rs:127:        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
crates/nmp-core/src/store/mem/tests.rs:130:        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
docs/design/lmdb/keys.md:17:| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
docs/design/lmdb/keys.md:24:| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `BTreeSet<EventId>` | pinned set per ClaimerId (deduped); rebuilt on restart from open views |
docs/design/lmdb/keys.md:63:Enables newest-first scans across **all authors** for a `(kind, d_tag)` pair — the use case is "find the most recent article with slug `my-post` across all authors" (kind:30023 global search). This is distinct from `idx_kind_dtag` which is exact-key by author.
docs/design/lmdb/keys.md:81:Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).
docs/design/lmdb/keys.md:179:4. `idx_kind_time.put(0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])` (only if `index_kind1_globally`; default off).
crates/nmp-core/src/store/mem/mod.rs:14://!   gc.rs       — claim / release / prune
crates/nmp-core/src/store/mem/mod.rs:35:/// Default maximum pinned events per view (D8 / gc.md §2).
crates/nmp-core/src/store/mem/mod.rs:38:/// Hard global pinned ceiling (D8 / gc.md §2).
crates/nmp-core/src/store/mem/mod.rs:39:pub(super) const MAX_PINNED_TOTAL: usize = 20_000;
crates/nmp-core/src/store/mem/mod.rs:76:    /// Claim budgets: claimer → max pinned.
crates/nmp-core/src/store/mem/mod.rs:77:    pub(super) claim_budgets: HashMap<ClaimerId, usize>,
crates/nmp-core/src/store/mem/mod.rs:79:    /// Current claims: claimer → BTreeSet of hex event ids.
crates/nmp-core/src/store/mem/mod.rs:80:    /// BTreeSet gives idempotency per T25 — re-claiming a known id is a no-op.
crates/nmp-core/src/store/mem/mod.rs:81:    pub(super) claims: HashMap<ClaimerId, BTreeSet<String>>,
crates/nmp-core/src/store/mem/mod.rs:94:            claim_budgets: HashMap::new(),
crates/nmp-core/src/store/mem/mod.rs:95:            claims: HashMap::new(),
crates/nmp-core/src/store/mem/gc.rs:3://! Implements the HotSet semantics from `docs/design/lmdb/gc.md` §2:
crates/nmp-core/src/store/mem/gc.rs:5://!   - global pinned ceiling: `MAX_PINNED_TOTAL` (20000 events).
crates/nmp-core/src/store/mem/gc.rs:6://!   - BTreeSet idempotency per T25: re-claiming a known id is a no-op.
crates/nmp-core/src/store/mem/gc.rs:7://!   - `StoreError::OverPinned` on breach (D8).
crates/nmp-core/src/store/mem/gc.rs:17:    claimer: ClaimerId,
crates/nmp-core/src/store/mem/gc.rs:21:    st.claim_budgets.insert(claimer, cover_budget);
crates/nmp-core/src/store/mem/gc.rs:25:pub(super) fn claim(
crates/nmp-core/src/store/mem/gc.rs:27:    claimer: ClaimerId,
crates/nmp-core/src/store/mem/gc.rs:31:    let ceiling = *st.claim_budgets.get(&claimer).unwrap_or(&DEFAULT_VIEW_CEILING);
crates/nmp-core/src/store/mem/gc.rs:33:    let existing_set = st.claims.entry(claimer).or_default();
crates/nmp-core/src/store/mem/gc.rs:41:    let current_for_claimer = existing_set.len();
crates/nmp-core/src/store/mem/gc.rs:42:    let requested_for_claimer = current_for_claimer + new_ids.len();
crates/nmp-core/src/store/mem/gc.rs:43:    if requested_for_claimer > ceiling {
crates/nmp-core/src/store/mem/gc.rs:45:            claimer,
crates/nmp-core/src/store/mem/gc.rs:46:            requested: requested_for_claimer,
crates/nmp-core/src/store/mem/gc.rs:51:    // Global pinned ceiling check (D8 / gc.md §2).
crates/nmp-core/src/store/mem/gc.rs:52:    let all_pinned: usize = st.claims.values().map(|s| s.len()).sum();
crates/nmp-core/src/store/mem/gc.rs:53:    let global_new = new_ids
crates/nmp-core/src/store/mem/gc.rs:55:        .filter(|hex| !st.claims.values().any(|s| s.contains(*hex)))
crates/nmp-core/src/store/mem/gc.rs:57:    let requested_global = all_pinned + global_new;
crates/nmp-core/src/store/mem/gc.rs:58:    if requested_global > MAX_PINNED_TOTAL {
crates/nmp-core/src/store/mem/gc.rs:60:            claimer,
crates/nmp-core/src/store/mem/gc.rs:61:            requested: requested_global,
crates/nmp-core/src/store/mem/gc.rs:66:    // Apply the claims.
crates/nmp-core/src/store/mem/gc.rs:67:    let set = st.claims.entry(claimer).or_default();
crates/nmp-core/src/store/mem/gc.rs:76:    claimer: ClaimerId,
crates/nmp-core/src/store/mem/gc.rs:79:    st.claims.remove(&claimer);
crates/nmp-core/src/store/mem/gc.rs:159:    fn claim_idempotent_reclaim_does_not_count() {
crates/nmp-core/src/store/mem/gc.rs:164:        store.claim(c, &[id]).unwrap();
crates/nmp-core/src/store/mem/gc.rs:165:        // Re-claiming the same id must not count toward the ceiling.
crates/nmp-core/src/store/mem/gc.rs:166:        store.claim(c, &[id]).unwrap();
crates/nmp-core/src/store/mem/gc.rs:168:        assert_eq!(st.claims[&c].len(), 1, "idempotent: re-claim must not add entry");
crates/nmp-core/src/store/mem/gc.rs:172:    fn claim_over_per_view_ceiling_returns_err() {
crates/nmp-core/src/store/mem/gc.rs:176:        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
crates/nmp-core/src/store/mem/gc.rs:177:        let result = store.claim(c, &[make_id(3)]);
crates/nmp-core/src/store/mem/gc.rs:189:        store.claim(c, &[make_id(1), make_id(2)]).unwrap();
crates/nmp-core/src/store/mem/gc.rs:192:        assert!(!st.claims.contains_key(&c), "release must clear claimer's pins");
docs/design/lmdb/trait/types.md:172:    /// Returned by `claim()` when the per-view or global pinned ceiling is exceeded.
docs/design/lmdb/trait/types.md:173:    /// The claim is rejected; no pin is written. The caller must release some
docs/design/lmdb/trait/types.md:174:    /// existing claim or reduce the requested set before retrying.
docs/design/lmdb/trait/types.md:175:    /// Maps to D8 (reactivity contract): a working-set overflow must be surfaced
docs/design/lmdb/trait/types.md:177:    #[error("claim ceiling exceeded: claimer={claimer:?} requested={requested} ceiling={ceiling}")]
docs/design/lmdb/trait/types.md:178:    OverPinned { claimer: ClaimerId, requested: usize, ceiling: usize },
docs/design/lmdb/gc.md:10:claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
docs/design/lmdb/gc.md:11:                where each `claimer` is an open ViewHandle / open ActionHandle
docs/design/lmdb/gc.md:19:hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
docs/design/lmdb/gc.md:30:pub(crate) struct HotSet {
docs/design/lmdb/gc.md:31:    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
docs/design/lmdb/gc.md:34:    pinned: HashMap<EventId, u32>,                   // event_id → refcount
docs/design/lmdb/gc.md:35:    // Reverse map for cheap release(); BTreeSet ensures claim() is idempotent per claimer.
docs/design/lmdb/gc.md:36:    by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>,
docs/design/lmdb/gc.md:40:    // Ceilings (enforced on every claim() call — D8 / ADR-0001..0004).
docs/design/lmdb/gc.md:41:    max_claim_per_view: usize,   // default 1_000; callers may lower via register_view_cover
docs/design/lmdb/gc.md:42:    max_pinned_total: usize,     // default 20_000; hard cap on pinned.len()
docs/design/lmdb/gc.md:45:impl HotSet {
docs/design/lmdb/gc.md:46:    /// Record the budget for a view before its first claim. If not called, the
docs/design/lmdb/gc.md:47:    /// default `max_claim_per_view` applies. Calling it again with a lower budget
docs/design/lmdb/gc.md:48:    /// after claims have already been issued does *not* retroactively reject them;
docs/design/lmdb/gc.md:49:    /// the lower ceiling applies to future claim() calls.
docs/design/lmdb/gc.md:54:    /// Pin `ids` for `c`. Idempotent: re-claiming an id already in the claimer's set
docs/design/lmdb/gc.md:58:    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
docs/design/lmdb/gc.md:59:        let existing = self.by_claimer.get(&c);
docs/design/lmdb/gc.md:60:        // Collect into a BTreeSet to dedup both intra-call and against already-claimed ids.
docs/design/lmdb/gc.md:68:            .unwrap_or(self.max_claim_per_view);
docs/design/lmdb/gc.md:69:        let current_for_claimer = existing.map_or(0, |s| s.len());
docs/design/lmdb/gc.md:70:        if current_for_claimer + new_ids.len() > per_view_ceiling {
docs/design/lmdb/gc.md:72:                claimer: c,
docs/design/lmdb/gc.md:73:                requested: current_for_claimer + new_ids.len(),
docs/design/lmdb/gc.md:77:        let new_global = self.pinned.len() + new_ids.iter()
docs/design/lmdb/gc.md:78:            .filter(|id| !self.pinned.contains_key(id))
docs/design/lmdb/gc.md:80:        if new_global > self.max_pinned_total {
docs/design/lmdb/gc.md:82:                claimer: c,
docs/design/lmdb/gc.md:83:                requested: new_global,
docs/design/lmdb/gc.md:84:                ceiling: self.max_pinned_total,
docs/design/lmdb/gc.md:87:        let set = self.by_claimer.entry(c).or_default();
docs/design/lmdb/gc.md:90:            *self.pinned.entry(*id).or_insert(0) += 1;
docs/design/lmdb/gc.md:96:        if let Some(ids) = self.by_claimer.remove(&c) {
docs/design/lmdb/gc.md:98:                if let Some(rc) = self.pinned.get_mut(&id) {
docs/design/lmdb/gc.md:100:                    if *rc == 0 { self.pinned.remove(&id); }
docs/design/lmdb/gc.md:114:            // pop_lru returns oldest; skip pinned ones until we find an evictable.
docs/design/lmdb/gc.md:119:                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
docs/design/lmdb/gc.md:125:            // If every LRU entry is pinned, the overflow will not be resolved by
docs/design/lmdb/gc.md:126:            // trim() alone. The working-set budget enforcement in claim() is the
docs/design/lmdb/gc.md:138:- `max_claim_per_view`: 1 000 events per claimer. A view that tries to pin more returns `OverPinned`; the actor surfaces this as `Effect::ViewOverPinned` and releases the claim.
docs/design/lmdb/gc.md:139:- `max_pinned_total`: 20 000 events globally. Prevents many moderate-sized views from collectively overwhelming the working set (D8 / ADR-0003 gate).
docs/design/lmdb/gc.md:141:These defaults allow 100 active views × 200 pins each = 20 000 globally, within the ADR-0003 §5 memory accounting (10k LRU + 20k pinned overlay ≈ 90 MB, under the 100 MB gate).
docs/design/lmdb/gc.md:185:The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:
docs/design/lmdb/gc.md:189:3. `store.register_view_cover(claimer_id, cover_budget)` registers the budget ceiling for this view. `cover_budget` is `spec.max_cover_size()` (a per-view-module constant; defaults to 200 if unspecified).
docs/design/lmdb/gc.md:190:4. `store.claim(claimer_id, &cover_ids)` pins those events in hot. Returns `StoreError::OverPinned` if the registered budget is exceeded; the actor releases the claim and surfaces `Effect::ViewOverPinned`.
docs/design/lmdb/gc.md:191:5. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally. Because `by_claimer` uses `BTreeSet<EventId>`, re-claiming an already-pinned id is a no-op — the refcount in `pinned` is not double-incremented.
docs/design/lmdb/gc.md:195:1. `store.release(claimer_id)` drops every pin in one call.
docs/design/lmdb/gc.md:196:2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.
docs/design/lmdb/gc.md:198:Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.
docs/design/lmdb/gc.md:209:| Claim refcount maps (≤20k pinned + 10k LRU entries) | ~1 MB | `HashMap<EventId, u32>` + reverse `by_claimer: HashMap<ClaimerId, BTreeSet<EventId>>` + `view_budgets`; global ceiling 20k pins keeps this bounded |
docs/design/lmdb/gc.md:226:| LRU evicted a still-pinned event (bug) | `trim()` would have skipped it; if observed, log + invariant violation | Pin reinstated from `claims_meta`; fire `tracing::error!`; flagged as critical bug class to investigate |
docs/design/lmdb/gc.md:228:| `release()` called for unknown `ClaimerId` | `by_claimer.remove` returns None | Silent no-op; logged at debug; not a bug (idempotent close) |
docs/design/lmdb/gc.md:229:| `claim()` exceeds per-view or global ceiling | Per-view: `by_claimer[c].len() + new_ids.len() > view_budgets[c]`; global: `pinned.len() + new_unique > max_pinned_total` (both counts deduplicated) | Return `StoreError::OverPinned`; state unchanged; actor surfaces `Effect::ViewOverPinned` and calls `release(claimer_id)` |
docs/design/lmdb/gc.md:241:    pub claim_pinned_count: usize,
crates/nmp-core/src/store/lmdb.rs:184:        _claimer: ClaimerId,
crates/nmp-core/src/store/lmdb.rs:190:    fn claim(&self, _claimer: ClaimerId, _ids: &[EventId]) -> Result<(), StoreError> {
crates/nmp-core/src/store/lmdb.rs:194:    fn release(&self, _claimer: ClaimerId) -> Result<(), StoreError> {
docs/design/lmdb/tests.md:92:| Claim/release; GC drops un-claimed | `store_gc_claims.rs` | §2.10 below |
docs/design/lmdb/tests.md:125:File: `crates/nmp-testing/tests/store_gc_claims.rs`
docs/design/lmdb/tests.md:130:- Assert: 10 claimed events still present in hot; 40 unclaimed events evicted from LRU but still readable from disk.
docs/design/lmdb/tests.md:132:- Assert: previously claimed events now subject to LRU eviction.
crates/nmp-core/src/store/events.rs:262:    // ─── Hot-set / claims (GC) ───────────────────────────────────────────────
crates/nmp-core/src/store/events.rs:265:    fn register_view_cover(&self, claimer: ClaimerId, cover_budget: usize) -> Result<(), StoreError>;
crates/nmp-core/src/store/events.rs:268:    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
crates/nmp-core/src/store/events.rs:270:    /// Release all pins held by `claimer`.
crates/nmp-core/src/store/events.rs:271:    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
crates/nmp-core/src/store/types/gc.rs:3://! D8: GC ceiling defaults (1000 per-view, 20000 global pinned) are documented
crates/nmp-core/src/store/types/errors.rs:54:    /// Returned by `claim()` when the per-view or global pinned ceiling is exceeded.
crates/nmp-core/src/store/types/errors.rs:55:    /// D8 / GC ceiling invariant — see `docs/design/lmdb/gc.md` §2.
crates/nmp-core/src/store/types/errors.rs:57:        claimer: ClaimerId,
crates/nmp-core/src/store/types/errors.rs:74:            StoreError::OverPinned { claimer, requested, ceiling } =>
crates/nmp-core/src/store/types/errors.rs:75:                write!(f, "claim ceiling exceeded: claimer={claimer:?} requested={requested} ceiling={ceiling}"),
crates/nmp-core/src/store/types/events.rs:42:        self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
crates/nmp-core/src/store/types/events.rs:50:    /// NIP-16 ephemeral kinds: 20000–29999.
crates/nmp-core/src/store/types/events.rs:52:        (20_000..30_000).contains(&self.kind)

exec
/bin/zsh -lc "nl -ba docs/product-spec/overview-and-dx.md | sed -n '1,140p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Product Spec: Overview And Developer Experience
     2	
     3	[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)
     4	
     5	# Product Specification — Nostr Multi-Platform Framework
     6	
     7	> **Working name:** `nmp` (Nostr Multi-Platform). Final name TBD per `aim.md` §7.7. Crate names below use the `nmp-*` prefix; substitute when renamed.
     8	
     9	> **Status:** Draft 0, revised for ADR-0009 and ADR-0010. The kernel/module split is now architectural ground truth; product modules still graduate by the phased plan in [`docs/plan.md`](../plan.md).
    10	
    11	> **Required prior reading:** `docs/aim.md`, then `rmp-architecture-bible.md` upstream at `rust-multiplatform/rmp`.
    12	
    13	---
    14	
    15	## 1. Product summary
    16	
    17	A Cargo workspace shipping a Nostr-native **app kernel** (`nmp-core`), reusable **Nostr protocol modules** (`nmp-nip01`, `nmp-nip17`, `nmp-nip65`, etc.), app-owned extension modules, a codegen tool (`nmp gen modules`) that produces per-app concrete FFI enums/wrappers, FFI bindings for Swift/Kotlin/TypeScript, a wasm target, a scaffolding CLI, and reference platform shells.
    18	
    19	The kernel composes the `rust-nostr` crate family plus OS capability crates into a substrate. It owns actor runtime, verified event store, subscription planner, relay routing pipeline, signer/session plumbing, durable action ledger, domain-store substrate, typed view registry, capability bridge, platform shadow/codegen machinery, diagnostics, and test harnesses.
    20	
    21	The kernel does **not** own Profile, Timeline, Thread, Reactions, Conversation, Wallet, DM, Blossom, or app-specific domain concepts. Those live in reusable protocol modules or app crates. Platform code renders state and dispatches user intents — nothing else.
    22	
    23	The framework treats common Nostr-correctness failures (stale replaceable events, lost subscriptions, mis-routed publishes, double-publication, multi-account desync, leaked secrets across FFI, naive cache invalidation, withheld cached data, blocking-on-fetch UI patterns) as **product defects in the framework** rather than as developer mistakes. The public API is designed so that the wrong thing is hard to type.
    24	
    25	---
    26	
    27	## 1.5 Cardinal doctrines D0–D8
    28	
    29	See [`docs/product-spec/doctrine.md`](./doctrine.md) for the full text of all nine doctrines.
    30	
    31	Summary: D0–D5 are *policy* doctrines (user-facing semantics); D6–D8 are *substrate invariants* (runtime implementation constraints). Both are equally binding. Every API decision answers to at least one; conflicts resolve in the order D0 > D1 > … > D8.
    32	
    33	| # | Name | Kind |
    34	|---|------|------|
    35	| D0 | No app nouns in `nmp-core`; test surface gated behind `test-support` feature | Policy |
    36	| D1 | Best-effort rendering — render now, refine in place | Policy |
    37	| D2 | Negentropy first, REQ second | Policy |
    38	| D3 | Outbox routing automatic; manual relay selection is the opt-out | Policy |
    39	| D4 | Single writer per fact; caches derive | Policy |
    40	| D5 | Snapshots bounded by what's open | Policy |
    41	| D6 | Errors never cross FFI as exceptions | Substrate |
    42	| D7 | Capabilities report; never decide policy | Substrate |
    43	| D8 | Reactivity contract: composite reverse index · ≤60 Hz/view · working-set bounded; idle-tick emit gated on `changed_since_emit()` | Substrate |
    44	
    45	---
    46	
    47	## 2. Audience and use cases
    48	
    49	**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven and inexperienced developers who lack the protocol literacy to navigate Nostr's footguns unaided.
    50	
    51	**Secondary audience.** Existing Nostr client teams considering a port to Rust + multi-platform, who want a substrate they can compose rather than reimplement.
    52	
    53	**Tertiary audience.** Tooling, agent, and bot authors who want the framework's event store + actions + sync as a headless Rust library, without UI.
    54	
    55	**In scope.**
    56	
    57	- General-purpose social clients (timeline, threads, profiles, follows, reactions, reposts, quotes).
    58	- DM-first messengers (NIP-17 over NIP-44 + NIP-59).
    59	- Long-form publishing tools (NIP-23).
    60	- Wallets and zap-centric apps (NIP-47 / NIP-57 / NIP-60 / NIP-61).
    61	- Media-heavy clients (Blossom BUD-01/02).
    62	- List managers and curation tools.
    63	
    64	**Out of scope for v1.**
    65	
    66	- Relay implementations (we depend on `relay-builder` for tests; we do not ship a production relay).
    67	- New NIP authorship.
    68	- Game engines, AR, low-latency audio/video pipelines (the bible's Pika has these because it has voice/video calls; we do not adopt that scope).
    69	- Non-Nostr protocol support (Bluesky, ActivityPub, etc.).
    70	
    71	---
    72	
    73	## 3. Success criteria
    74	
    75	Acceptance is **demonstrable, not aspirational**. A claim that the framework works is provable by running these:
    76	
    77	### 3.1 Zero-to-running starter
    78	
    79	```bash
    80	nmp init my-app
    81	cd my-app && just run-ios   # works
    82	just run-android            # works
    83	just run-desktop            # works
    84	just run-web                # works
    85	```
    86	
    87	Result on each platform: a starter app with login (private key + NIP-46 bunker), a "following" timeline, compose, profile view, profile edit, and a DM inbox + thread. End-to-end build + first launch ≤ 5 minutes on a developer laptop with the framework's `nix develop` shell active, ≤ 15 minutes from cold without Nix.
    88	
    89	### 3.2 The "few hundred lines" test
    90	
    91	Across the four platform shells of the starter app, total non-generated platform code must fit within these budgets (excluding asset declarations and boilerplate `main`):
    92	
    93	| Platform | Budget (LOC, hand-written) |
    94	|----------|----------------------------|
    95	| iOS (SwiftUI) | ≤ 400 |
    96	| Android (Compose) | ≤ 400 |
    97	| Desktop (iced) | ≤ 600 (iced is more verbose; this is the bible's pattern) |
    98	| Web (wasm + TS/JSX shell) | ≤ 400 |
    99	
   100	Exceeding any budget is a framework-design failure: it means rendering logic is being forced to compensate for missing surface in the core.
   101	
   102	### 3.3 Bug class extinction
   103	
   104	Each of these classes must be structurally impossible to introduce through the safe framework public API. Lower-level Rust escape hatches used for tests or internal policy modules must be named, instrumented, and regression-tested. Each bug class below is paired with a regression test in `crates/nmp-testing`.
   105	
   106	1. Stale replaceable event (kind 0/3/10000-19999/30000-39999) retained in state after a newer one arrives.
   107	2. Subscription leaked after its UI is destroyed.
   108	3. Publish of an event to relays the author has not declared as write relays, without explicit override.
   109	4. DM published to public relays.
   110	5. Two account contexts having overlapping mutable state.
   111	6. Cache miss returning empty without triggering a fallback fetch.
   112	7. Profile-edit action that updates the local cache but fails to publish (or vice versa).
   113	8. Two concurrent UI subscriptions for the same filter producing two relay REQs.
   114	9. NIP-46 signing session lost on app suspend/resume without prompt.
   115	10. Re-published event missing its original `id` due to re-signing.
   116	
   117	Each test asserts the framework refuses the broken usage (compile-time, type-system, or runtime panic in debug) or routes around it transparently.
   118	
   119	### 3.4 LLM-friendliness
   120	
   121	A novice or LLM-driven developer, given only `docs/aim.md`, `docs/product-spec.md`, the generated bindings, and the starter app, can implement a new screen (e.g., "show all kind-1 events tagged with a given hashtag") that:
   122	
   123	- compiles on first try with no edits to the core,
   124	- correctly routes to the right relays automatically,
   125	- correctly closes its subscriptions when navigated away from,
   126	- correctly handles cache misses and live updates.
   127	
   128	We treat this as a property of the spec: if it fails repeatedly with capable LLMs, the API surface is wrong, not the LLM.
   129	
   130	### 3.5 Cross-platform consistency
   131	
   132	A scripted action sequence (defined in `crates/nmp-testing`) run against the starter app on all four platforms produces byte-identical `AppState` JSON snapshots after each action. Divergence is a framework defect, not a platform issue.
   133	
   134	---
   135	
   136	## 4. Deliverables
   137	
   138	### 4.1 Workspace
   139	
   140	The on-disk layout from `aim.md` §5 is canonical. The long-term workspace contains the following crates as published artifacts on crates.io:

exec
/bin/zsh -lc "nl -ba docs/perf/firehose-bench/1779068929-live.md | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Firehose Bench Report
     2	
     3	- Status: `live`
     4	- Mode: `live`
     5	- Scale: `standard`
     6	- Started at unix: `1779068929`
     7	- Overall passed: `true`
     8	
     9	## Scenario Summary
    10	
    11	| Scenario | Events | Duration | Passed | Key metrics |
    12	|---|---:|---:|---|---|
    13	| cold_start | 200 | 60s | true | first_item=294.81ms |
    14	
    15	## Limitations
    16	
    17	- Live mode exercises cold_start and profile_thrashing against real relays (M1 scope).
    18	- Scenarios requiring LMDB, NIP-65 outbox, NIP-77, NIP-42 auth, multi-account, NSE, or soak are not implemented for M1.
    19	
    20	## Observations
    21	

exec
/bin/zsh -lc "nl -ba docs/perf/firehose-bench/1779070804-live.md | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Firehose Bench Report
     2	
     3	- Status: `live`
     4	- Mode: `live`
     5	- Scale: `standard`
     6	- Started at unix: `1779070804`
     7	- Overall passed: `true`
     8	
     9	## Scenario Summary
    10	
    11	| Scenario | Events | Duration | Passed | Key metrics |
    12	|---|---:|---:|---|---|
    13	| cold_start | 200 | 60s | true | first_item=383.81ms |
    14	
    15	## Limitations
    16	
    17	- Live mode exercises cold_start and profile_thrashing against real relays (M1 scope).
    18	- Scenarios requiring LMDB, NIP-65 outbox, NIP-77, NIP-42 auth, multi-account, NSE, or soak are not implemented for M1.
    19	
    20	## Observations
    21	

exec
/bin/zsh -lc 'git show --format=fuller --no-patch 7d16b3f && git show --format=fuller --no-patch bc84cfe' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
commit 7d16b3f248c0df396c741323b99d09bf5a9b48e3
Author:     pablof7z <pfer@me.com>
AuthorDate: Mon May 18 05:21:40 2026 +0300
Commit:     pablof7z <pfer@me.com>
CommitDate: Mon May 18 05:21:49 2026 +0300

    fix(m3): codex follow-up — mem.rs split + sig verify + ingest wired + GC ceiling + tombstones (T29)
    
    P1 — Article I hard-ceiling fix:
    - Delete monolithic store/mem.rs (1105 LOC) → split into mem/{mod,insert,query,gc,domain,store_impl,tests}.rs (all ≤394 LOC)
    - Delete store/types.rs (343 LOC) → split into types/{ids,events,outcomes,watermark,gc,errors,mod}.rs (all ≤80 LOC)
    
    P1 — D4 single-writer fix:
    - Remove #[allow(dead_code)] on kernel store field
    - Add sig: String (serde default) to NostrEvent so relay events parse correctly
    - ingest_timeline_event now routes through self.store.insert() with real relay URL
    
    P2 — verify sigs (nostr crate wired):
    - Add nostr = "0.44" dep to nmp-core
    - VerifiedEvent newtype in store/types/events.rs: try_from_raw() calls nostr::Event::verify()
    - from_raw_unchecked() escape hatch under cfg(any(test, feature = "test-support"))
    - VerifyError in store/types/errors.rs (InvalidId, InvalidSignature, Serialization)
    - EventStore::insert now takes VerifiedEvent — callers must verify before inserting
    - kernel/ingest: replace "0".repeat(128) placeholder with VerifiedEvent::try_from_raw(); invalid-sig events logged + dropped
    
    P2 — tombstone max-merge fix:
    - merge_tombstone() takes max(deleted_at) and unions sources across re-deliveries
    
    P2 — dup provenance fix:
    - Exact-id duplicate checked BEFORE kind-specific supersession in handle_supersession()
    
    P2 — D8 GC ceilings:
    - claims field is BTreeSet<String> for idempotency
    - gc::claim() enforces per-view ceiling (1000) and global pinned ceiling (20000)
    - StoreError::OverPinned returned on breach
    
    P3 — LMDB honesty: already compliant (all methods return Err(not_enabled()))
    
    Bench gates (M1 live cold_start):
    - first_item=294.81ms ≤ 800ms gate PASS (run 1779068929)
    - first_item=383.81ms ≤ 800ms gate PASS (run 1779070804, post VerifiedEvent)
    
    profile_thrashing live bench: snapshot_valid=false failure is pre-existing on origin/master
    (confirmed by running against stashed kernel — same failure before any T29 changes)
    
    Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
commit bc84cfe4b76fcf3c1bfeb1456fffc25634181036
Author:     pablof7z <pfer@me.com>
AuthorDate: Mon May 18 05:23:29 2026 +0300
Commit:     pablof7z <pfer@me.com>
CommitDate: Mon May 18 05:23:45 2026 +0300

    fix(m2): codex follow-up — Rule 1 wildcard + plan-id stability + lane modeling + file splits + TODO removal (T30)
    
    P1 — Rule 1 wildcard bug (lattice/mod.rs): empty kinds = wildcard; wildcard ∪
    {1,6} now correctly returns wildcard (empty set), not {1,6}. Updated the
    existing rule1_wildcard_absorbs_specific test (which codified the bug) and
    added wildcard_unions_with_anything_stays_wildcard covering the negative-direction.
    
    P1 — RoutingSource::Indexer removed (plan.rs): added UserConfiguredCategory
    enum (AccountRead / AccountWrite / Indexer / Debug) so indexer fallback is
    RoutingSource::UserConfigured(UserConfiguredCategory::Indexer) — lane 4, not a
    fifth lane. Matches diagnostics.md §5.0 + ADR-0007 four-lane discipline. Updated
    compiler and audit gate.
    
    P1 — Plan-id binding fixed (compiler/plan_id.rs): hash now covers only
    referenced pubkeys (authors ∪ addresses.pubkey ∪ #p tag values), not the
    entire mailbox cache. Added CompileContext with indexer_set_version and
    user_config_version. Sort all relay vectors before hashing. Include scope in
    hash. Three new tests: plan_id_unchanged_when_unrelated_mailbox_arrives,
    plan_id_changes_when_referenced_author_mailbox_updates,
    plan_id_changes_on_indexer_set_version_bump (in m2_plan_id_stability.rs).
    
    P2 — Direction table complete (compiler/partition.rs): added Case C (#p Inbox
    direction with structural ban on non-inbox routes) and Case D (active-account
    read relays for no-author hashtag firehose). IndexerProbe seam via
    MailboxCache::request_probe() default no-op method.
    SubscriptionCompiler::with_active_account_read_relays() constructor.
    
    P2 — Audit strength: 300 → 1000 authors (Assertion 2). Assertion 5 now also
    verifies merged address set is the union and both originating_interests tracked.
    
    P2 — mod.rs minimized: submodules pub(crate); public surface narrowed to the
    listed types. External audit test updated to import from planner:: re-exports.
    
    P3 — File splits: compiler.rs (710 LOC) → compiler/{mod,mailbox,plan_id,partition}.rs
    (191/105/159/258 LOC). lattice.rs (483 LOC) → lattice/{mod,rules}.rs (389/136 LOC).
    m2_subscription_compilation_audit.rs → audit (460) + m2_plan_id_stability (225).
    
    P3 — TODO removal: compiler.rs:183 TODO(phase2) → prose comment. interest.rs
    TODO(nmp-nip19) → "Phase 2" prefix. plan.rs TODO(wire-emitter) → section header.
    requests/{profile,thread}.rs TODO(M2-migration) → "M2 migration plan" headers.
    
    cargo test --workspace: 100% pass (19 lattice + 9 audit + 3 plan-id stability).
    cargo clippy --workspace --all-targets -- -D warnings: clean.
    firehose-bench replay --standard --fail-on-gate: overall_passed=true.
    
    Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/mod.rs | sed -n '1,100p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! Subscription compiler — the M2 planner subsystem.
     2	//!
     3	//! Turns a set of `LogicalInterest`s into a `CompiledPlan` mapping each
     4	//! relay URL to the exact set of REQ frames to emit.
     5	//!
     6	//! ## Module structure
     7	//!
     8	//! - `interest`  — `LogicalInterest`, `InterestShape`, `NaddrCoord` types.
     9	//! - `lattice`   — `merge()` function implementing the 8 merge rules.
    10	//! - `compiler`  — 4-stage pipeline: resolve → fallback → merge → plan-id.
    11	//! - `plan`      — `CompiledPlan`, `RelayPlan`, `SubShape`, `RoutingSource`.
    12	//!
    13	//! ## Usage (phase 1)
    14	//!
    15	//! ```rust,ignore
    16	//! use nmp_core::planner::{
    17	//!     compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
    18	//!     interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
    19	//! };
    20	//!
    21	//! let cache = InMemoryMailboxCache::new();
    22	//! let indexer = vec!["wss://purplepag.es".to_string()];
    23	//! let compiler = SubscriptionCompiler::new(&cache, &indexer);
    24	//! let plan = compiler.compile(&[interest])?;
    25	//! ```
    26	//!
    27	//! ## Doctrine compliance
    28	//!
    29	//! - **D3** — outbox routing is automatic; view modules never name relay URLs.
    30	//! - **D6** — `PlannerError` is an internal `Result`; it never crosses FFI.
    31	//!   Map to a toast string at the actor boundary.
    32	//! - **D8** — the hot path (merge lattice) uses only stack-allocated comparisons
    33	//!   after the initial interest registration.
    34	//!
    35	//! Design: `docs/design/subscription-compilation/`
    36	
    37	pub(crate) mod compiler;
    38	pub(crate) mod interest;
    39	pub(crate) mod lattice;
    40	pub(crate) mod plan;
    41	
    42	// ─── Public API surface ──────────────────────────────────────────────────────
    43	//
    44	// Only the items below cross the crate boundary. Internals (RelayEntry,
    45	// partition_interest, FnvHasher, rule*_* functions, etc.) stay module-private.
    46	// `lattice::merge` is re-exported for the nmp-testing audit gate; all others
    47	// are consumed by crate-internal callers (kernel, actor).
    48	
    49	pub use compiler::{
    50	    CompileContext,
    51	    EmptyMailboxCache,
    52	    InMemoryMailboxCache,
    53	    MailboxCache,
    54	    MailboxSnapshot,
    55	    SubscriptionCompiler,
    56	};
    57	pub use interest::{
    58	    InterestId,
    59	    InterestLifecycle,
    60	    InterestScope,
    61	    InterestShape,
    62	    LogicalInterest,
    63	    NaddrCoord,
    64	    Pubkey,
    65	    RelayUrl,
    66	};
    67	pub use lattice::MergeOutcome;
    68	pub use plan::{CompiledPlan, PlannerError, RelayPlan, RoutingSource, SubShape, UserConfiguredCategory};

exec
/bin/zsh -lc "rg -n \"planner::merge|\\bmerge\\(\" crates/nmp-testing crates/nmp-core/src/planner -S" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
crates/nmp-core/src/planner/compiler/mod.rs:11://!    with `lattice::merge()` (Rules 1–8). Author sets are partitioned per
crates/nmp-core/src/planner/compiler/mod.rs:147:                        merge(&existing_shape.clone(), &shape, existing_lifecycle, &lifecycle)
crates/nmp-core/src/planner/mod.rs:9://! - `lattice`   — `merge()` function implementing the 8 merge rules.
crates/nmp-core/src/planner/lattice/mod.rs:1://! The filter-merge lattice: `merge()` implements Rules 1–8 from the compiler
crates/nmp-core/src/planner/lattice/mod.rs:49:pub fn merge(
crates/nmp-core/src/planner/lattice/mod.rs:141:        let r = merge(&a, &b, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:149:        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:159:        let r = merge(&a, &b, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:178:            let r_ab = merge(&wildcard, &concrete, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:179:            let r_ba = merge(&concrete, &wildcard, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:190:        let r = merge(&wildcard, &wildcard, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:207:        let r = merge(&a, &b, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:224:        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:233:        let r = merge(&a, &b, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:245:        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:254:        let r = merge(&a, &b, &tailing(), &tailing());
crates/nmp-core/src/planner/lattice/mod.rs:266:        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:275:        assert!(matches!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Merged(_)));
crates/nmp-core/src/planner/lattice/mod.rs:282:        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:286:        assert_eq!(merge(&c, &d, &tailing(), &tailing()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:295:        assert!(matches!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Merged(_)));
crates/nmp-core/src/planner/lattice/mod.rs:296:        assert!(matches!(merge(&a, &b, &one_shot(), &one_shot()), MergeOutcome::Merged(_)));
crates/nmp-core/src/planner/lattice/mod.rs:303:        assert_eq!(merge(&a, &b, &tailing(), &one_shot()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:318:        let r = merge(&a, &b, &one_shot(), &one_shot());
crates/nmp-core/src/planner/lattice/mod.rs:333:        assert_eq!(merge(&a, &b, &one_shot(), &one_shot()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/mod.rs:352:        let r = merge(&a, &b, &one_shot(), &one_shot());
crates/nmp-core/src/planner/lattice/mod.rs:370:        let r = merge(&a, &b, &one_shot(), &one_shot());
crates/nmp-core/src/planner/lattice/mod.rs:387:        assert_eq!(merge(&a, &b, &tailing(), &one_shot()), MergeOutcome::Refused);
crates/nmp-core/src/planner/lattice/rules.rs:4://! All rules are `pub(super)` — only the lattice `merge()` entry point is public.

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/compiler/plan_id.rs | sed -n '1,220p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/compiler/partition.rs | sed -n '1,280p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! Plan-id hashing: `CompileContext` and `compute_plan_id`.
     2	//!
     3	//! The plan-id is a content-addressed string that uniquely identifies a
     4	//! compiled plan. It covers only the inputs that actually affect routing:
     5	//! referenced pubkeys (not the full mailbox cache), interest shapes, scopes,
     6	//! and version counters.
     7	//!
     8	//! Design: `docs/design/subscription-compilation/compiler.md` §3.4
     9	//! Doctrine: D8 (plan-id stability avoids redundant recompilation).
    10	
    11	use std::collections::BTreeSet;
    12	use crate::planner::interest::{InterestLifecycle, InterestScope, LogicalInterest, Pubkey};
    13	use super::mailbox::MailboxCache;
    14	
    15	// ─── CompileContext ───────────────────────────────────────────────────────────
    16	
    17	/// Versioning inputs for plan-id binding (§3.4).
    18	///
    19	/// Both counters advance whenever the corresponding policy changes:
    20	/// - `indexer_set_version` — bumped when the kernel's indexer relay set changes.
    21	/// - `user_config_version` — bumped when user-configured relay settings change.
    22	///
    23	/// Including these in the plan-id hash ensures that plan-ids invalidate when
    24	/// policy changes even if the interest set itself is unchanged.
    25	///
    26	/// Design: `docs/design/subscription-compilation/compiler.md` §3.4
    27	#[derive(Clone, Debug, Default)]
    28	pub struct CompileContext {
    29	    /// Monotonic counter advancing on every accepted change to the indexer set.
    30	    pub indexer_set_version: u64,
    31	    /// Monotonic counter advancing on every accepted change to user-configured relays.
    32	    pub user_config_version: u64,
    33	}
    34	
    35	// ─── FNV-1a hasher ───────────────────────────────────────────────────────────
    36	
    37	/// FNV-1a hasher (64-bit).
    38	///
    39	/// Phase 1 implementation. Phase 2 will upgrade to blake3 when that crate
    40	/// joins the workspace.
    41	struct FnvHasher(u64);
    42	
    43	impl FnvHasher {
    44	    fn new() -> Self {
    45	        Self(0xcbf29ce484222325)
    46	    }
    47	    fn feed_bytes(&mut self, bytes: &[u8]) {
    48	        for &b in bytes {
    49	            self.0 ^= u64::from(b);
    50	            self.0 = self.0.wrapping_mul(0x100000001b3);
    51	        }
    52	    }
    53	    fn feed_u64(&mut self, v: u64) {
    54	        self.feed_bytes(&v.to_le_bytes());
    55	    }
    56	    fn finish(self) -> u64 {
    57	        self.0
    58	    }
    59	}
    60	
    61	// ─── Referenced pubkeys ───────────────────────────────────────────────────────
    62	
    63	/// Collect all pubkeys that are referenced by the interest set.
    64	///
    65	/// Per §3.4: only the mailbox entries for **referenced** pubkeys participate
    66	/// in the plan-id hash. An unrelated kind:10002 arrival (for a pubkey not in
    67	/// any interest's author set, #p tags, or address pubkeys) MUST NOT change
    68	/// the plan-id.
    69	///
    70	/// Referenced pubkeys = `interest.shape.authors ∪ addresses[*].pubkey ∪ tags["p"][*]`
    71	pub(super) fn referenced_pubkeys(interests: &[LogicalInterest]) -> BTreeSet<Pubkey> {
    72	    let mut pks = BTreeSet::new();
    73	    for interest in interests {
    74	        pks.extend(interest.shape.authors.iter().cloned());
    75	        for coord in &interest.shape.addresses {
    76	            pks.insert(coord.pubkey.clone());
    77	        }
    78	        if let Some(p_values) = interest.shape.tags.get("p") {
    79	            pks.extend(p_values.iter().cloned());
    80	        }
    81	    }
    82	    pks
    83	}
    84	
    85	// ─── compute_plan_id ─────────────────────────────────────────────────────────
    86	
    87	/// Compute a stable, deterministic plan-id string.
    88	///
    89	/// Hash inputs (all sorted for determinism):
    90	/// 1. Sorted interests: id + shape (JSON) + scope + lifecycle.
    91	/// 2. Mailbox snapshot for ONLY referenced pubkeys (§3.4 stability rule).
    92	///    Relay vectors within each snapshot are sorted before hashing.
    93	/// 3. Compile context: `indexer_set_version` + `user_config_version`.
    94	/// 4. Merge lattice version.
    95	///
    96	/// An unrelated kind:10002 arrival (for a pubkey not in any interest's author
    97	/// set / #p tags / address pubkeys) MUST NOT change the plan-id.
    98	pub(super) fn compute_plan_id(
    99	    interests: &[LogicalInterest],
   100	    cache: &dyn MailboxCache,
   101	    ctx: &CompileContext,
   102	    lattice_version: u8,
   103	) -> String {
   104	    let mut h = FnvHasher::new();
   105	
   106	    // ── 1. Sorted interest contributions ─────────────────────────────────────
   107	    let mut sorted_interests: Vec<&LogicalInterest> = interests.iter().collect();
   108	    sorted_interests.sort_by_key(|i| &i.id);
   109	    for interest in sorted_interests {
   110	        h.feed_u64(interest.id.0);
   111	        if let Ok(shape_json) = serde_json::to_vec(&interest.shape) {
   112	            h.feed_bytes(&shape_json);
   113	        }
   114	        let scope_tag: u8 = match &interest.scope {
   115	            InterestScope::ActiveAccount => 0,
   116	            InterestScope::Account(acct) => {
   117	                h.feed_bytes(acct.as_bytes());
   118	                1
   119	            }
   120	            InterestScope::Global => 2,
   121	        };
   122	        h.feed_bytes(&[scope_tag]);
   123	        let lifecycle_tag: u8 = match &interest.lifecycle {
   124	            InterestLifecycle::Tailing => 0,
   125	            InterestLifecycle::OneShot => 1,
   126	            InterestLifecycle::BoundedTime { until_ms } => {
   127	                h.feed_u64(*until_ms);
   128	                2
   129	            }
   130	        };
   131	        h.feed_bytes(&[lifecycle_tag]);
   132	    }
   133	
   134	    // ── 2. Mailbox snapshot — referenced pubkeys only ─────────────────────────
   135	    let ref_pks = referenced_pubkeys(interests);
   136	    for pk in &ref_pks {
   137	        if let Some(mb) = cache.get(pk) {
   138	            h.feed_bytes(pk.as_bytes());
   139	            let mut write_sorted = mb.write_relays.clone();
   140	            write_sorted.sort();
   141	            for r in &write_sorted { h.feed_bytes(r.as_bytes()); }
   142	            let mut read_sorted = mb.read_relays.clone();
   143	            read_sorted.sort();
   144	            for r in &read_sorted { h.feed_bytes(r.as_bytes()); }
   145	            let mut both_sorted = mb.both_relays.clone();
   146	            both_sorted.sort();
   147	            for r in &both_sorted { h.feed_bytes(r.as_bytes()); }
   148	        }
   149	    }
   150	
   151	    // ── 3. Compile context ────────────────────────────────────────────────────
   152	    h.feed_u64(ctx.indexer_set_version);
   153	    h.feed_u64(ctx.user_config_version);
   154	
   155	    // ── 4. Lattice version ────────────────────────────────────────────────────
   156	    h.feed_bytes(&[lattice_version]);
   157	
   158	    format!("{:016x}", h.finish())
   159	}

 succeeded in 0ms:
     1	//! `RelayEntry` and `partition_interest`: Stage 1+2 of the compiler pipeline.
     2	//!
     3	//! Partitions a single `LogicalInterest` into per-relay entries, with each
     4	//! entry carrying only the authors that declared the relay (author-partitioning).
     5	//!
     6	//! Design: `docs/design/subscription-compilation/compiler.md` §3.1 / §3.2
     7	//! Doctrine: D3 (outbox routing automatic).
     8	
     9	use std::collections::{BTreeMap, BTreeSet};
    10	
    11	use crate::planner::{
    12	    interest::{
    13	        InterestId, InterestLifecycle, InterestShape, LogicalInterest, NaddrCoord, Pubkey,
    14	        RelayUrl,
    15	    },
    16	    plan::{RoutingSource, UserConfiguredCategory},
    17	};
    18	use super::mailbox::MailboxCache;
    19	
    20	// ─── RelayEntry ──────────────────────────────────────────────────────────────
    21	
    22	/// A relay-partitioned slice of one logical interest.
    23	///
    24	/// When an interest has N authors, Stage 1 produces one `RelayEntry` per
    25	/// `(relay, interest_id)` pair, where `authors_for_relay` contains only the
    26	/// authors that declared this specific relay (not all N authors). This is the
    27	/// author-partitioning that lets the merge lattice produce per-relay author
    28	/// subsets.
    29	pub(super) struct RelayEntry {
    30	    /// The interest's non-author fields (kinds, tags, since, until, etc.).
    31	    /// `authors` is intentionally left empty here; we merge `authors_for_relay`
    32	    /// in at Stage 3 merge time.
    33	    pub base_shape: InterestShape,
    34	    /// The subset of authors from this interest that declared this relay.
    35	    pub authors_for_relay: BTreeSet<Pubkey>,
    36	    /// Address-pointer coordinates from this interest (if relevant for routing).
    37	    pub addresses_for_relay: BTreeSet<NaddrCoord>,
    38	    pub lifecycle: InterestLifecycle,
    39	    pub source: RoutingSource,
    40	    pub interest_id: InterestId,
    41	}
    42	
    43	impl RelayEntry {
    44	    /// Construct the final `InterestShape` for this relay slice.
    45	    pub fn into_shape(mut self) -> (InterestShape, InterestLifecycle, RoutingSource, InterestId) {
    46	        self.base_shape.authors = self.authors_for_relay;
    47	        self.base_shape.addresses = self.addresses_for_relay;
    48	        (self.base_shape, self.lifecycle, self.source, self.interest_id)
    49	    }
    50	}
    51	
    52	// ─── partition_interest ───────────────────────────────────────────────────────
    53	
    54	/// Stage 1 + 2: partition one logical interest into per-relay entries.
    55	///
    56	/// Each entry carries only the AUTHORS that declared the specific relay,
    57	/// preserving per-relay author-subset semantics (Assertion 2, §3.3).
    58	///
    59	/// ## Direction routing (§3.1 / §3.2)
    60	///
    61	/// - **Case A**: explicit `authors` → Outbox (write relays). Also routes
    62	///   any `addresses` on the same interest to the same relay map.
    63	/// - **Case B**: no authors, but `addresses` → Outbox for each coord.pubkey.
    64	/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
    65	///   (tagged pubkey's read relays). Structural ban enforced: never route
    66	///   private `#p` interests to non-inbox relays.
    67	///   Phase 1 stub: falls back to indexer; real inbox resolution in phase 2.
    68	/// - **Case D (no-author)**: no authors, addresses, or #p → active-account
    69	///   read relays (hashtag firehose, global search). Falls to indexer if empty.
    70	pub(super) fn partition_interest(
    71	    interest: &LogicalInterest,
    72	    mailbox_cache: &dyn MailboxCache,
    73	    indexer_relays: &[RelayUrl],
    74	    active_account_read_relays: &[RelayUrl],
    75	    relay_entries: &mut BTreeMap<RelayUrl, Vec<RelayEntry>>,
    76	) {
    77	    let base_shape = InterestShape {
    78	        authors: BTreeSet::new(),
    79	        kinds: interest.shape.kinds.clone(),
    80	        tags: interest.shape.tags.clone(),
    81	        since: interest.shape.since,
    82	        until: interest.shape.until,
    83	        limit: interest.shape.limit,
    84	        event_ids: interest.shape.event_ids.clone(),
    85	        addresses: BTreeSet::new(),
    86	    };
    87	
    88	    // Case A: explicit authors → Outbox (write relays).
    89	    if !interest.shape.authors.is_empty() {
    90	        let mut per_relay: BTreeMap<RelayUrl, (BTreeSet<Pubkey>, BTreeSet<NaddrCoord>, RoutingSource)> =
    91	            BTreeMap::new();
    92	
    93	        for author in &interest.shape.authors {
    94	            match mailbox_cache.get(author) {
    95	                Some(snapshot) => {
    96	                    for relay in snapshot.outbox_relays() {
    97	                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
    98	                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
    99	                        });
   100	                        entry.0.insert(author.clone());
   101	                    }
   102	                }
   103	                None => {
   104	                    for relay in indexer_relays {
   105	                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
   106	                            (BTreeSet::new(), BTreeSet::new(),
   107	                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
   108	                        });
   109	                        entry.0.insert(author.clone());
   110	                    }
   111	                }
   112	            }
   113	        }
   114	
   115	        for coord in &interest.shape.addresses {
   116	            match mailbox_cache.get(&coord.pubkey) {
   117	                Some(snapshot) => {
   118	                    for relay in snapshot.outbox_relays() {
   119	                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
   120	                            (BTreeSet::new(), BTreeSet::new(), RoutingSource::Nip65)
   121	                        });
   122	                        entry.1.insert(coord.clone());
   123	                    }
   124	                }
   125	                None => {
   126	                    for relay in indexer_relays {
   127	                        let entry = per_relay.entry(relay.clone()).or_insert_with(|| {
   128	                            (BTreeSet::new(), BTreeSet::new(),
   129	                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
   130	                        });
   131	                        entry.1.insert(coord.clone());
   132	                    }
   133	                }
   134	            }
   135	        }
   136	
   137	        for (relay_url, (authors, addrs, source)) in per_relay {
   138	            relay_entries.entry(relay_url).or_default().push(RelayEntry {
   139	                base_shape: base_shape.clone(),
   140	                authors_for_relay: authors,
   141	                addresses_for_relay: addrs,
   142	                lifecycle: interest.lifecycle.clone(),
   143	                source,
   144	                interest_id: interest.id.clone(),
   145	            });
   146	        }
   147	        return;
   148	    }
   149	
   150	    // Case B: no explicit authors, but address-pointer pubkeys → Outbox.
   151	    if !interest.shape.addresses.is_empty() {
   152	        let mut per_relay_addrs: BTreeMap<RelayUrl, (BTreeSet<NaddrCoord>, RoutingSource)> =
   153	            BTreeMap::new();
   154	
   155	        for coord in &interest.shape.addresses {
   156	            match mailbox_cache.get(&coord.pubkey) {
   157	                Some(snapshot) => {
   158	                    for relay in snapshot.outbox_relays() {
   159	                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
   160	                            (BTreeSet::new(), RoutingSource::Nip65)
   161	                        });
   162	                        entry.0.insert(coord.clone());
   163	                    }
   164	                }
   165	                None => {
   166	                    for relay in indexer_relays {
   167	                        let entry = per_relay_addrs.entry(relay.clone()).or_insert_with(|| {
   168	                            (BTreeSet::new(),
   169	                             RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
   170	                        });
   171	                        entry.0.insert(coord.clone());
   172	                    }
   173	                }
   174	            }
   175	        }
   176	
   177	        for (relay_url, (addrs, source)) in per_relay_addrs {
   178	            relay_entries.entry(relay_url).or_default().push(RelayEntry {
   179	                base_shape: base_shape.clone(),
   180	                authors_for_relay: BTreeSet::new(),
   181	                addresses_for_relay: addrs,
   182	                lifecycle: interest.lifecycle.clone(),
   183	                source,
   184	                interest_id: interest.id.clone(),
   185	            });
   186	        }
   187	        return;
   188	    }
   189	
   190	    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
   191	    //
   192	    // #p interests (DMs, notifications) MUST route to the tagged pubkey's READ
   193	    // relays (Inbox direction). Routing them to write relays violates the
   194	    // structural ban on private routes to non-inbox relays (§3.2).
   195	    //
   196	    // Phase 1 stub: read_relays not yet populated from kind:10002 → fall back
   197	    // to indexer. The code path is correct; only the mailbox data is missing.
   198	    let p_tag_values: BTreeSet<Pubkey> = interest
   199	        .shape
   200	        .tags
   201	        .get("p")
   202	        .cloned()
   203	        .unwrap_or_default();
   204	
   205	    if !p_tag_values.is_empty() {
   206	        for tagged_pk in &p_tag_values {
   207	            match mailbox_cache.get(tagged_pk) {
   208	                Some(snapshot) if !snapshot.read_relays.is_empty() => {
   209	                    for relay in &snapshot.read_relays {
   210	                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
   211	                            base_shape: base_shape.clone(),
   212	                            authors_for_relay: BTreeSet::new(),
   213	                            addresses_for_relay: BTreeSet::new(),
   214	                            lifecycle: interest.lifecycle.clone(),
   215	                            source: RoutingSource::Nip65,
   216	                            interest_id: interest.id.clone(),
   217	                        });
   218	                    }
   219	                }
   220	                _ => {
   221	                    mailbox_cache.request_probe(tagged_pk);
   222	                    for relay in indexer_relays {
   223	                        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
   224	                            base_shape: base_shape.clone(),
   225	                            authors_for_relay: BTreeSet::new(),
   226	                            addresses_for_relay: BTreeSet::new(),
   227	                            lifecycle: interest.lifecycle.clone(),
   228	                            source: RoutingSource::UserConfigured(
   229	                                UserConfiguredCategory::Indexer,
   230	                            ),
   231	                            interest_id: interest.id.clone(),
   232	                        });
   233	                    }
   234	                }
   235	            }
   236	        }
   237	        return;
   238	    }
   239	
   240	    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
   241	    let (fallback_relays, fallback_source) = if !active_account_read_relays.is_empty() {
   242	        (active_account_read_relays,
   243	         RoutingSource::UserConfigured(UserConfiguredCategory::AccountRead))
   244	    } else {
   245	        (indexer_relays,
   246	         RoutingSource::UserConfigured(UserConfiguredCategory::Indexer))
   247	    };
   248	    for relay in fallback_relays {
   249	        relay_entries.entry(relay.clone()).or_default().push(RelayEntry {
   250	            base_shape: base_shape.clone(),
   251	            authors_for_relay: BTreeSet::new(),
   252	            addresses_for_relay: BTreeSet::new(),
   253	            lifecycle: interest.lifecycle.clone(),
   254	            source: fallback_source.clone(),
   255	            interest_id: interest.id.clone(),
   256	        });
   257	    }
   258	}

exec
/bin/zsh -lc "nl -ba docs/design/subscription-compilation/compiler.md | sed -n '1,180p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Subscription Compilation §3 — The Compilation Pipeline
     2	
     3	> Parent: `docs/design/subscription-compilation.md`.
     4	> Read first: [intro.md](intro.md) for the `LogicalInterest` shape this stage consumes.
     5	
     6	The compiler is a pure function plus a small amount of state (the mailbox cache and the active plan registry). It runs whenever a recompilation trigger fires (§4) and produces a `CompiledPlan` that the wire-emitter applies as a diff against the relay sockets.
     7	
     8	## 3.0 Pipeline overview
     9	
    10	```
    11	   logical_interests:                 mailbox_cache + relay_config
    12	   Vec<LogicalInterest>                       │
    13	            │                                 │
    14	            ▼                                 ▼
    15	   ┌───────────────────────────────────────────────────┐
    16	   │ Stage 1: Resolve authors → mailboxes              │  (§3.1)
    17	   │   each author → { write, read, both, missing }    │
    18	   └───────────────────────────────────────────────────┘
    19	            │
    20	            ▼
    21	   ┌───────────────────────────────────────────────────┐
    22	   │ Stage 2: Indexer fallback for missing mailboxes   │  (§3.2)
    23	   │   missing → enqueue kind:10002 probe              │
    24	   │   missing-author reads → indexer set (read only)  │
    25	   └───────────────────────────────────────────────────┘
    26	            │
    27	            ▼
    28	   ┌───────────────────────────────────────────────────┐
    29	   │ Stage 3: Per-relay shape merge                    │  (§3.3)
    30	   │   group interests by target relay URL             │
    31	   │   merge compatible shapes inside each relay       │
    32	   │   refuse merges that would change semantics       │
    33	   └───────────────────────────────────────────────────┘
    34	            │
    35	            ▼
    36	   ┌───────────────────────────────────────────────────┐
    37	   │ Stage 4: Plan-id binding                          │  (§3.4)
    38	   │   compute plan_id = hash(interest_set,            │
    39	   │                          mailbox_snapshot,        │
    40	   │                          merge_lattice_version)   │
    41	   │   stable across no-op recompilations              │
    42	   └───────────────────────────────────────────────────┘
    43	            │
    44	            ▼
    45	   CompiledPlan { plan_id, per_relay: Vec<RelayPlan> }
    46	```
    47	
    48	The wire-emitter (`crates/nmp-core/src/kernel/wire.rs`, to be added) diffs the new plan against the current wire-sub registry: opens new REQs, closes orphaned ones, leaves stable assignments untouched.
    49	
    50	## 3.1 Stage 1 — Resolve authors to mailboxes
    51	
    52	Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).
    53	
    54	Output: an `AuthorRouting` per author per direction:
    55	
    56	```rust
    57	pub struct AuthorRouting {
    58	    pub author: Pubkey,
    59	    pub direction: RoutingDirection,        // Outbox or Inbox
    60	    pub relays: BTreeSet<RelayUrl>,         // resolved write/read/both
    61	    pub source: RoutingSource,              // Nip65 | UserConfigured | Hint
    62	    // Note: there is no RoutingSource::Indexer variant — indexer fallback is
    63	    // modeled as UserConfigured { category: Indexer } (see diagnostics.md §5.0).
    64	    // This keeps the four-lane discipline strict: indexers are a sub-category of
    65	    // user-configured, not a fifth lane.
    66	    pub freshness_ms: Option<u64>,          // age of the kind:10002 record
    67	}
    68	
    69	pub enum RoutingDirection {
    70	    Outbox,    // for `authors:` filters — the author's *write* relays
    71	    Inbox,     // for `#p:` filters    — the tagged author's *read* relays
    72	}
    73	```
    74	
    75	Direction is decided by the interest's filter shape per `docs/product-spec/subsystems.md` §7.3:
    76	
    77	| Interest shape | Direction | Source per author |
    78	|---|---|---|
    79	| Non-empty `authors`, no `#p` | Outbox | author's `write_relays ∪ both_relays` |
    80	| Empty `authors`, non-empty `#p` | Inbox | tagged author's `read_relays ∪ both_relays` |
    81	| Both populated | Outbox primarily; Inbox interests split (see §3.3) | both |
    82	| Neither populated | (handled by stage 3 as "use active-account read relays") | — |
    83	
    84	`docs/product-spec/subsystems.md` §7.3 specifies one explicit override: DMs (NIP-17 gift-wraps, M9) fail closed if recipient inbox relays are missing. The compiler enforces this by refusing to produce a plan for an interest tagged `privacy = FailClosed` if any tagged-pubkey inbox lookup has empty relays or was sourced from `UserConfigured { category: Indexer }` (meaning no NIP-65-declared inbox exists). §7 details the publish-side enforcement.
    85	
    86	## 3.2 Stage 2 — Indexer fallback for unknown mailboxes
    87	
    88	The indexer set is a kernel-configured `Vec<RelayUrl>` (default: a small curated list; user-configurable in `AppConfig`). Today's `crates/nmp-core/src/relay.rs:2` is the placeholder for one indexer relay (`purplepag.es`); the v1 indexer set lives in `AppConfig.indexer_relays`.
    89	
    90	Two distinct behaviours:
    91	
    92	1. **Mailbox probe.** For every author with `mailbox_cache.get(author) == None`, the compiler emits a `IndexerProbe { author }` side effect on the plan. The probe registers as its own short-lived `LogicalInterest { shape: { kinds: [10002], authors: [author], limit: 1 }, lifecycle: OneShot, scope: Global }`. Recompilation triggers (§4 trigger A1) re-route the original interest once the kind:10002 lands.
    93	2. **Read fallback.** For a `RoutingDirection::Outbox` interest whose author has no known mailboxes, the compiler routes the interest to the indexer set **as read-only fallback**. Per `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." The resulting `AuthorRouting` carries `source: RoutingSource::UserConfigured` with a `UserConfiguredRelayFact { category: UserConfiguredCategory::Indexer }` record flowing to the diagnostic surface. The four-lane view (§5) renders "author X is being served by indexer Y because we have no mailbox for them" by examining the `category` subcategory — the Indexer is a sub-category of lane 4 (User-configured), not a fifth lane.
    94	
    95	Bounded: a single author's indexer probe is enqueued at most once per `compiler_probe_window_secs` (default 60s) to prevent thundering-herd probes if a screen of N rows all claim the same unknown pubkey.
    96	
    97	## 3.3 Stage 3 — Per-relay shape merge
    98	
    99	After Stage 1, every interest has one or more `(relay_url, sub_shape)` assignments. Stage 3 groups by relay URL and merges shapes where merging preserves semantics.
   100	
   101	### Merge rules (the lattice)
   102	
   103	Two `InterestShape`s `A` and `B` are **mergeable on relay R** iff:
   104	
   105	1. `A.kinds == B.kinds` **or** one is empty (wildcard absorbs).
   106	2. `A.tags.keys() == B.tags.keys()` (same tag dimensions) **and** the union of values per dimension stays under the relay's per-filter limit (default 1000).
   107	3. `A.since` and `B.since`: merged `since = min(A.since, B.since)` *only if* both are present or both absent. Mixing a bounded interest with an unbounded one is **not** merged (would broaden the bounded one's window).
   108	4. `A.until` and `B.until`: same rule, mirror of (3) with `max`.
   109	5. `A.limit` and `B.limit`: mergeable iff both are absent. If either has a `limit`, **do not merge** — broadening would mask the limit's intent.
   110	6. `A.lifecycle == B.lifecycle`. Tailing and one-shot do not merge (one-shot would never close).
   111	7. `A.event_ids` and `B.event_ids`: merge by union, capped at the relay's per-filter `ids` limit.
   112	8. **Rule 8 (address-pointer union).** `A.addresses` and `B.addresses` merge by `A.addresses ∪ B.addresses`, provided their other constraints (`authors`, `kinds`, `tags`, time, lifecycle) merge per Rules 1–7. Overlapping coordinates with differing time or lifecycle constraints do **not** merge — the compiler emits per-address sub-shapes so each `NaddrCoord`'s routing is independent. The union cap is the relay's per-filter `#a` value limit (default 1000). Address routing uses `NaddrCoord::pubkey` as the `AuthorRouting` input (Outbox direction, Stage 1), so the relays chosen are the addressed author's write relays — the same path as an `authors`-bearing filter for that pubkey.
   113	
   114	When mergeable, the merged shape is `{ authors: A.authors ∪ B.authors, addresses: A.addresses ∪ B.addresses, ... }`. The merged interest tracks both originating `InterestId`s so per-event dispatch back to consumers stays correct.
   115	
   116	When not mergeable, the two interests get distinct sub-shapes on the same relay, producing two distinct REQs. That is fine and expected.
   117	
   118	Open question 2 in the parent index (`subscription-compilation.md`) covers the `limit`-only corner case formally.
   119	
   120	The NMP merge lattice is simpler than Applesauce's `selectOptimalRelays` greedy set-cover
   121	(`docs/research/applesauce/outbox.md` §3, `relay-selection.ts:14-93`): Applesauce optimizes
   122	the number of relay connections by picking a minimum covering set across all contacts. NMP's
   123	Stage 3 merges shapes per relay but does not eliminate relays — the set-cover optimization
   124	(capping to `maxConnections`) is a future extension. For M2, every declared write relay gets
   125	a REQ; relay-count optimization is open question 8 (future ADR).
   126	
   127	### Per-relay output
   128	
   129	```rust
   130	pub struct RelayPlan {
   131	    pub relay_url: RelayUrl,
   132	    pub role_tags: BTreeSet<RoutingSource>,   // why this relay is in the plan
   133	    pub sub_shapes: Vec<SubShape>,            // each emits one REQ
   134	}
   135	
   136	pub struct SubShape {
   137	    pub shape: InterestShape,                  // canonical, post-merge
   138	    pub originating_interests: Vec<InterestId>,
   139	    pub canonical_filter_hash: String,         // for ADR-0007 WireSubscriptionStatus
   140	}
   141	```
   142	
   143	The wire-emitter renders each `SubShape` as exactly one `REQ` on `relay_url` with a sub-id of `c{plan_id}-r{relay_idx}-s{shape_idx}`. The sub-id is meaningful only to the kernel; diagnostics use `canonical_filter_hash` for stable identity across re-emission.
   144	
   145	## 3.4 Stage 4 — Plan-id binding
   146	
   147	`plan_id` is the **stable identity** the platform observes for diagnostic continuity. It answers: "did this recompilation actually change anything observable?"
   148	
   149	Definition: **hash only mailboxes referenced by the current interest set**, not the whole
   150	cache. This resolves open question 1 in the parent index — choosing this scope means that
   151	an unrelated author's kind:10002 arriving does not churn plan-ids for unrelated interests.
   152	
   153	```
   154	// referenced_pubkeys = union of all shape.authors and shape.tags[#p] across interest_set
   155	referenced_pubkeys = interest_set.iter()
   156	    .flat_map(|i| i.shape.authors.iter().chain(i.shape.tags.get("#p").unwrap_or(&[])))
   157	    .collect::<BTreeSet<Pubkey>>();
   158	
   159	plan_id = blake3(
   160	    sorted(interest_set.iter().map(|i| (i.id, i.shape, i.scope, i.lifecycle))),
   161	    sorted(referenced_pubkeys.iter().filter_map(|pk| mailbox_cache.get(pk))
   162	              .map(|ml| (ml.pubkey, ml.created_at, sorted(ml.write), sorted(ml.read)))),
   163	    INDEXER_SET_VERSION,
   164	    USER_CONFIGURED_RELAYS_VERSION,
   165	    MERGE_LATTICE_VERSION,
   166	)
   167	```
   168	
   169	Properties:
   170	
   171	- **Recompilation with no change ⇒ same plan-id.** If `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:218-221`) deduplicates and decides not to replace a stale mailbox, no plan-id churn.
   172	- **Adding an interest changes plan-id even if no new wire REQ results.** Two interests can merge into the same SubShape; the plan-id changes because the *interest set* changed. The platform diagnostic correctly reports "logical-interest count went up; wire-sub count did not."
   173	- **A new kind:10002 for a referenced author changes plan-id.** The hash only covers mailboxes for authors the current interest set touches, so a kind:10002 for an unrelated pubkey (not in any active interest's `authors` or `#p`) does not churn plan-ids.
   174	- **A new kind:10002 for an unreferenced author does NOT change plan-id.** This is the key
   175	  difference from hashing the whole snapshot. The wire-emitter's diff is unaffected. This
   176	  property is critical for D8 (reactivity contract — §1.5): recompilation work must be bounded
   177	  by what's open, not by the size of the total mailbox cache. Hashing only referenced mailboxes
   178	  ensures recompile cost scales with `|interest_set| × |referenced_authors|`, not with the
   179	  entire cache.
   180	- **Indexer set change changes plan-id.** Operator config edits surface immediately.

exec
/bin/zsh -lc 'rg -n "#p|Inbox|structural ban|IndexerProbe|active-account|read relays|direction" docs/design/subscription-compilation/compiler.md docs/design/subscription-compilation/diagnostics.md docs/decisions/0007* -S' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/design/subscription-compilation/compiler.md:52:Inputs: every `LogicalInterest` with non-empty `shape.authors` or non-empty `shape.tags[#p]`; the mailbox cache populated by `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`).
docs/design/subscription-compilation/compiler.md:54:Output: an `AuthorRouting` per author per direction:
docs/design/subscription-compilation/compiler.md:59:    pub direction: RoutingDirection,        // Outbox or Inbox
docs/design/subscription-compilation/compiler.md:71:    Inbox,     // for `#p:` filters    — the tagged author's *read* relays
docs/design/subscription-compilation/compiler.md:79:| Non-empty `authors`, no `#p` | Outbox | author's `write_relays ∪ both_relays` |
docs/design/subscription-compilation/compiler.md:80:| Empty `authors`, non-empty `#p` | Inbox | tagged author's `read_relays ∪ both_relays` |
docs/design/subscription-compilation/compiler.md:81:| Both populated | Outbox primarily; Inbox interests split (see §3.3) | both |
docs/design/subscription-compilation/compiler.md:82:| Neither populated | (handled by stage 3 as "use active-account read relays") | — |
docs/design/subscription-compilation/compiler.md:92:1. **Mailbox probe.** For every author with `mailbox_cache.get(author) == None`, the compiler emits a `IndexerProbe { author }` side effect on the plan. The probe registers as its own short-lived `LogicalInterest { shape: { kinds: [10002], authors: [author], limit: 1 }, lifecycle: OneShot, scope: Global }`. Recompilation triggers (§4 trigger A1) re-route the original interest once the kind:10002 lands.
docs/design/subscription-compilation/compiler.md:112:8. **Rule 8 (address-pointer union).** `A.addresses` and `B.addresses` merge by `A.addresses ∪ B.addresses`, provided their other constraints (`authors`, `kinds`, `tags`, time, lifecycle) merge per Rules 1–7. Overlapping coordinates with differing time or lifecycle constraints do **not** merge — the compiler emits per-address sub-shapes so each `NaddrCoord`'s routing is independent. The union cap is the relay's per-filter `#a` value limit (default 1000). Address routing uses `NaddrCoord::pubkey` as the `AuthorRouting` input (Outbox direction, Stage 1), so the relays chosen are the addressed author's write relays — the same path as an `authors`-bearing filter for that pubkey.
docs/design/subscription-compilation/compiler.md:154:// referenced_pubkeys = union of all shape.authors and shape.tags[#p] across interest_set
docs/design/subscription-compilation/compiler.md:156:    .flat_map(|i| i.shape.authors.iter().chain(i.shape.tags.get("#p").unwrap_or(&[])))
docs/design/subscription-compilation/compiler.md:173:- **A new kind:10002 for a referenced author changes plan-id.** The hash only covers mailboxes for authors the current interest set touches, so a kind:10002 for an unrelated pubkey (not in any active interest's `authors` or `#p`) does not churn plan-ids.
docs/design/subscription-compilation/compiler.md:193:| `open_firehose_tag` (requests.rs:170-200) | Registers one `LogicalInterest { shape: { kinds: [1], tags: { #t: [tag] } }, scope: ActiveAccount, lifecycle: Tailing }`. Routes to active-account read relays per §3.1 table. |
docs/design/subscription-compilation/diagnostics.md:119:    AccountRead,                                // user's own read relays (overrides NIP-65 read)

exec
/bin/zsh -lc "nl -ba docs/design/subscription-compilation/diagnostics.md | sed -n '1,130p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Subscription Compilation §5 + §8 — Four-Lane Diagnostics
     2	
     3	> Parent: `docs/design/subscription-compilation.md`.
     4	> Read first: ADR-0007 (`docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md`) — every record here extends ADR-0007 types; it does not replace them.
     5	
     6	The compiler's routing decisions are the most subtle correctness surface in the M2 milestone. They are also the easiest to silently get wrong (`docs/design/ndk-applesauce-lessons.md` §3, "automatic behaviour also needs strong tests"). Diagnostics make the four sources of relay knowledge legible — separately, never collapsed.
     7	
     8	**Indexer fallback is lane 4 (User-configured), not a fifth lane.** The kernel-configured
     9	indexer set is an operator policy choice expressed as `UserConfiguredCategory::Indexer` (see
    10	§5.1 Lane 4). Keeping it inside lane 4 preserves the four-lane discipline and ensures that
    11	the diagnostic UI always sees exactly four columns, regardless of whether an author is
    12	being served via NIP-65, hints, provenance, or any subcategory of user-configured (including
    13	indexers). This resolves the ambiguity at prior diagnostics.md lines 15, 116, 157.
    14	
    15	## 5.0 The four lanes
    16	
    17	Per `docs/design/ndk-applesauce-lessons.md` §4 (lines 39–46) and `docs/aim.md` §6 doctrine 10 ("provenance preserved"), the four relay-fact lanes are:
    18	
    19	1. **NIP-65** — a pubkey's declared relay preferences (kind:10002).
    20	2. **Hint** — relay URLs embedded in events or NIP-19 pointers (`e`/`a` tag third slot, `nevent`'s relay vector, etc.).
    21	3. **Provenance** — relays we have actually observed an event arriving from.
    22	4. **User-configured** — local-policy relays added by the user/operator, plus the kernel-configured indexer fallback set.
    23	
    24	Each lane is its own record stream. They never merge into a single "relays" field — that collapse is exactly the bug `docs/design/ndk-applesauce-lessons.md` §4 line 46 forbids. They may be displayed side-by-side in a diagnostic view; the actor stores them apart.
    25	
    26	This is structurally enforced: there is no `Vec<RelayUrl>` field on any compiler output type. Every relay-bearing field carries a `lane: RelayFactLane` discriminator.
    27	
    28	```rust
    29	// crates/nmp-core/src/kernel/diagnostics/lanes.rs (proposed)
    30	
    31	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    32	pub enum RelayFactLane {
    33	    Nip65,
    34	    Hint,
    35	    Provenance,
    36	    UserConfigured,
    37	}
    38	```
    39	
    40	## 5.1 Per-lane record schemas
    41	
    42	Each lane has one record type. All four are exposed to the platform via the existing ADR-0007 `ViewBatch` lane (low-cadence, coalesced to 1–4 Hz per ADR-0007 "How status crosses the bridge"). They feed into the diagnostics screen, not into normal product UI.
    43	
    44	### Lane 1 — `Nip65RelayFact`
    45	
    46	```rust
    47	pub struct Nip65RelayFact {
    48	    pub pubkey: Pubkey,
    49	    pub relay_url: RelayUrl,
    50	    pub roles: Nip65Roles,                    // read | write | both
    51	    pub kind10002_event_id: EventId,           // provenance of the kind:10002
    52	    pub kind10002_created_at: UnixSeconds,
    53	    pub kind10002_seen_from: Vec<RelayUrl>,    // which relays delivered it
    54	    pub freshness: FreshnessHint,              // recent / hours_old / days_old / never_verified
    55	}
    56	
    57	pub struct Nip65Roles {
    58	    pub read: bool,
    59	    pub write: bool,
    60	}
    61	```
    62	
    63	Emitted whenever `ingest_relay_list` (`crates/nmp-core/src/kernel/ingest.rs:209-233`) replaces a mailbox entry. One record per `(pubkey, relay_url)` pair; an author with 4 declared relays produces 4 records on each update.
    64	
    65	### Lane 2 — `HintRelayFact`
    66	
    67	```rust
    68	pub struct HintRelayFact {
    69	    /// The pubkey this hint is associated with — the event author for EventTag hints,
    70	    /// the pointer's subject for Nip19 hints. Required for the coverage reducer to
    71	    /// count how many authors are served via hints (see §8.2 by_lane.hint counter).
    72	    pub subject: Pubkey,
    73	    pub relay_url: RelayUrl,
    74	    pub source: HintSource,
    75	    pub freshness_ms: u64,                     // monotonic from observation
    76	    pub recently_succeeded: bool,              // last attempt produced ≥1 EVENT
    77	}
    78	
    79	pub enum HintSource {
    80	    EventTag    { event_id: EventId, tag: TagKey, position: u8 },
    81	    Nip19       { pointer: String /* nevent1.../naddr1... */ },
    82	    UserConfig  { config_path: String },        // for hints injected via config
    83	}
    84	```
    85	
    86	The `subject` field is the author identity key for the coverage reducer's `by_lane.hint`
    87	counter (§8.2). Without it, the reverse-relay-coverage view cannot answer "how many distinct
    88	authors are routed via hints to relay R?" — the assertion in `tests.md:202` that
    89	`coverage.by_lane.hint == 1` would be untestable.
    90	
    91	Emitted by the pointer loader (post-M2; for M2 the field exists but is rarely populated — only `e`/`a`-tag third-slot hints from thread-view hydration fill it). Per-event hints are de-duplicated; an event whose `e` tag contains a hint URL produces one `HintRelayFact` per (relay_url, source) pair.
    92	
    93	### Lane 3 — `ProvenanceRelayFact`
    94	
    95	```rust
    96	pub struct ProvenanceRelayFact {
    97	    pub relay_url: RelayUrl,
    98	    pub event_id: EventId,
    99	    pub seen_at_ms: u64,
   100	    pub wire_sub_id: String,                   // which REQ delivered it
   101	    pub kind: u32,
   102	    pub author: Pubkey,
   103	}
   104	```
   105	
   106	Emitted by `handle_event` (`crates/nmp-core/src/kernel/ingest.rs:134-164`) for every EVENT arrival. This is the highest-cardinality lane and the only one where coalescing matters at the ADR-0007 boundary: the platform diagnostic view consumes a summarised projection (`ProvenanceSummary` per author or per event), not the raw fact stream.
   107	
   108	### Lane 4 — `UserConfiguredRelayFact`
   109	
   110	```rust
   111	pub struct UserConfiguredRelayFact {
   112	    pub relay_url: RelayUrl,
   113	    pub category: UserConfiguredCategory,
   114	    pub generation: u64,                       // config version; matches Trigger::*Changed
   115	    pub added_at_ms: u64,
   116	}
   117	
   118	pub enum UserConfiguredCategory {
   119	    AccountRead,                                // user's own read relays (overrides NIP-65 read)
   120	    AccountWrite,                               // user's own write relays
   121	    /// Kernel-configured indexer relay (e.g. purplepag.es). This is the sub-category that
   122	    /// represents indexer fallback routing in diagnostics — NOT a fifth lane. The indexer
   123	    /// set is a policy choice that lives inside lane 4 (User-configured). D3: the operator
   124	    /// configured the indexer set; the kernel applies it as policy for reads when NIP-65
   125	    /// mailboxes are unknown. Never for writes.
   126	    Indexer,
   127	    Debug,                                      // operator-injected for testing
   128	}
   129	```
   130	

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/interest.rs | sed -n '1,140p' && rg -n \"tags\\.insert|tags:|\\\"p\\\"|\\\"#p\\\"\" crates/nmp-core crates/nmp-testing docs/design/subscription-compilation -S" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! `LogicalInterest`, `InterestShape`, and `NaddrCoord` types.
     2	//!
     3	//! A logical interest is what a kernel-side consumer (view, action, monitor,
     4	//! sync job, or pointer loader) wants alive on the wire. The compiler in
     5	//! `planner::compiler` turns N logical interests into M ≤ N per-relay plans.
     6	//!
     7	//! Design: `docs/design/subscription-compilation/intro.md` §2.1
     8	//! Doctrine: D3 (outbox routing automatic), D6 (errors are internal Results),
     9	//!           D8 (composite reverse index, zero per-event allocs after warmup).
    10	
    11	use serde::{Deserialize, Serialize};
    12	use std::collections::{BTreeMap, BTreeSet};
    13	
    14	// ─── Type aliases (lightweight; no nostr-sdk dep) ────────────────────────────
    15	
    16	/// Hex-encoded 64-char pubkey.
    17	pub type Pubkey = String;
    18	
    19	/// Hex-encoded 64-char event id.
    20	pub type EventId = String;
    21	
    22	/// A `wss://` URL for a relay.
    23	pub type RelayUrl = String;
    24	
    25	/// Unix timestamp in seconds.
    26	pub type UnixSeconds = u64;
    27	
    28	/// A Nostr tag key (e.g. "e", "p", "t", "a").
    29	pub type TagKey = String;
    30	
    31	// ─── InterestId ──────────────────────────────────────────────────────────────
    32	
    33	/// Stable identity assigned by the planner registry on first insertion.
    34	/// Two interests with identical content get distinct ids if registered by
    35	/// distinct claims (the registry is the authority, not content hashing).
    36	#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
    37	pub struct InterestId(pub u64);
    38	
    39	// ─── NaddrCoord ──────────────────────────────────────────────────────────────
    40	
    41	/// A parameterized-replaceable event coordinate: the triple that uniquely
    42	/// identifies an addressable event (kinds 10000–19999, 30000–39999) across
    43	/// all relays. Equivalent to the `naddr` bech32 encoding without the relay hint.
    44	///
    45	/// Used by `InterestShape::addresses` for address-pointer hydration (Rule 8
    46	/// of the merge lattice) and by the D8 composite reverse index to deduplicate
    47	/// address-pointer interests across views.
    48	///
    49	/// Design: `docs/design/subscription-compilation/intro.md` §2.1
    50	#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
    51	pub struct NaddrCoord {
    52	    /// Author of the addressed event.
    53	    pub pubkey: Pubkey,
    54	    /// Addressable kind (10000–19999 or 30000–39999).
    55	    pub kind: u32,
    56	    /// The `d` tag value; empty string for events with no `d` tag.
    57	    pub d_tag: String,
    58	}
    59	
    60	// Phase 2 (nmp-nip19): NaddrCoord::from_naddr_bech32 / to_naddr_bech32 helpers
    61	// land when the nmp-nip19 bech32 codec crate joins the workspace. Both helpers
    62	// are needed for the ThreadViewModule and MetaTimelineViewModule address-pointer
    63	// loaders to accept user-facing naddr strings from the Swift/Kotlin FFI surface.
    64	
    65	// ─── InterestShape ───────────────────────────────────────────────────────────
    66	
    67	/// The normalised filter description for a `LogicalInterest`.
    68	///
    69	/// Mirrors the Nostr filter shape closely. All collections use sorted-container
    70	/// types so equality and hashing are deterministic — required for plan-id
    71	/// stability across recompilations (§3.4 plan-id contract).
    72	///
    73	/// Empty collections mean "wildcard" except where noted.
    74	#[derive(Clone, Debug, Default, Hash, Eq, PartialEq, Serialize, Deserialize)]
    75	pub struct InterestShape {
    76	    /// Authors whose events are wanted. Empty = any author (rare; prefer scoped).
    77	    pub authors: BTreeSet<Pubkey>,
    78	
    79	    /// Event kinds wanted. Empty = any kind (rare).
    80	    pub kinds: BTreeSet<u32>,
    81	
    82	    /// Tag filter dimensions. Each entry is a tag key → sorted set of values.
    83	    /// Sorted for hash stability (D8 composite index invariant).
    84	    pub tags: BTreeMap<TagKey, BTreeSet<String>>,
    85	
    86	    /// Lower bound for `created_at`. `None` = no lower bound.
    87	    pub since: Option<UnixSeconds>,
    88	
    89	    /// Upper bound for `created_at`. `None` = no upper bound.
    90	    pub until: Option<UnixSeconds>,
    91	
    92	    /// Maximum events to return. `None` = relay default.
    93	    /// When set, merge is refused (broadening would mask intent). See Rule 5.
    94	    pub limit: Option<u32>,
    95	
    96	    /// Specific event ids for pointer / thread hydration.
    97	    pub event_ids: BTreeSet<EventId>,
    98	
    99	    /// Parameterized-replaceable event coordinates for address-pointer hydration.
   100	    ///
   101	    /// Non-empty when a view needs to resolve a specific `naddr` (e.g., a NIP-23
   102	    /// article in `ThreadViewModule` or `MetaTimelineViewModule`). The compiler
   103	    /// routes each coordinate to the addressed author's write relays (Stage 1
   104	    /// Outbox direction keyed on `NaddrCoord::pubkey`). See Rule 8 and §7 of
   105	    /// the design doc.
   106	    ///
   107	    /// Adding `addresses` as a first-class field gives the merge lattice a stable
   108	    /// key to union on, rather than encoding coords into opaque `#a` tag strings.
   109	    ///
   110	    /// Design: `docs/design/subscription-compilation/intro.md` §2.1 (T24).
   111	    pub addresses: BTreeSet<NaddrCoord>,
   112	}
   113	
   114	impl InterestShape {
   115	    /// Convenience constructor for a tailing author+kind timeline interest.
   116	    pub fn timeline_for(authors: BTreeSet<Pubkey>) -> Self {
   117	        Self {
   118	            authors,
   119	            kinds: [1u32, 6u32].into_iter().collect(),
   120	            ..Default::default()
   121	        }
   122	    }
   123	
   124	    /// Convenience constructor for a one-shot profile fetch.
   125	    pub fn profile_for(pubkey: Pubkey) -> Self {
   126	        Self {
   127	            authors: [pubkey].into_iter().collect(),
   128	            kinds: [0u32].into_iter().collect(),
   129	            limit: Some(1),
   130	            ..Default::default()
   131	        }
   132	    }
   133	}
   134	
   135	// ─── InterestLifecycle ───────────────────────────────────────────────────────
   136	
   137	/// Controls when the compiler's wire-emitter closes the REQ.
   138	#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
   139	pub enum InterestLifecycle {
   140	    /// Stay open after EOSE (tailing subscription).
crates/nmp-testing/src/store_harness.rs:74:            tags: vec![],
crates/nmp-testing/src/store_harness.rs:93:            tags: vec![],
crates/nmp-testing/src/store_harness.rs:105:        tags: Vec<Vec<String>>,
crates/nmp-core/src/store/mem/tests.rs:29:            tags: vec![vec!["e".into(), target_hex.clone()]],
crates/nmp-core/src/store/mem/tests.rs:41:            tags: vec![vec!["e".into(), target_hex.clone()]],
crates/nmp-core/src/store/mem/tests.rs:66:            tags: vec![],
crates/nmp-core/src/store/types/events.rs:24:    pub tags: Vec<Vec<String>>,
crates/nmp-core/src/store/types/events.rs:86:            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
crates/nmp-core/src/kernel/tests.rs:45:            tags: vec![
crates/nmp-core/src/kernel/mod.rs:54:    tags: Vec<Vec<String>>,
crates/nmp-testing/bin/reactivity-bench/world.rs:276:                e_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:277:                p_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:288:                        e_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:289:                        p_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:299:                        e_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:300:                        p_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:313:                    e_tags: SmallTags::one(self.thread_root_id),
crates/nmp-testing/bin/reactivity-bench/world.rs:314:                    p_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:326:                    e_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:327:                    p_tags: SmallTags::empty(),
crates/nmp-testing/bin/reactivity-bench/world.rs:345:        let mut e_tags = SmallTags::empty();
crates/nmp-testing/bin/reactivity-bench/world.rs:354:            p_tags: SmallTags::empty(),
docs/design/subscription-compilation/nip65.md:208:pub fn parse_relay_list(created_at: UnixSeconds, tags: &[Vec<String>])
crates/nmp-core/src/kernel/requests/profile.rs:16://! - `open_firehose_tag`   → register LogicalInterest { kinds:[1], tags:{#t:[tag]} }
crates/nmp-testing/bin/reactivity-bench/domain.rs:10:    pub(crate) e_tags: SmallTags,
crates/nmp-testing/bin/reactivity-bench/domain.rs:11:    pub(crate) p_tags: SmallTags,
crates/nmp-core/src/kernel/ingest.rs:192:                if tag.first().map(String::as_str) == Some("p") {
crates/nmp-core/src/kernel/ingest.rs:256:            tags: event.tags.clone(),
crates/nmp-core/src/kernel/ingest.rs:300:            tags: event.tags,
crates/nmp-core/src/kernel/nostr.rs:9:    pub(super) tags: Vec<Vec<String>>,
crates/nmp-core/src/kernel/nostr.rs:101:pub(super) fn parse_relay_list(created_at: u64, tags: &[Vec<String>]) -> AuthorRelayList {
docs/design/subscription-compilation/compiler.md:132:    pub role_tags: BTreeSet<RoutingSource>,   // why this relay is in the plan
docs/design/subscription-compilation/compiler.md:156:    .flat_map(|i| i.shape.authors.iter().chain(i.shape.tags.get("#p").unwrap_or(&[])))
docs/design/subscription-compilation/compiler.md:193:| `open_firehose_tag` (requests.rs:170-200) | Registers one `LogicalInterest { shape: { kinds: [1], tags: { #t: [tag] } }, scope: ActiveAccount, lifecycle: Tailing }`. Routes to active-account read relays per §3.1 table. |
docs/design/subscription-compilation/intro.md:92:    pub tags:       BTreeMap<TagKey, BTreeSet<String>>,  // sorted for hash stability
docs/design/subscription-compilation/intro.md:143:- `ThreadView { event_id }` returns up to two interests: `{ ids: [...] }` for context, `{ kinds: {1, 6}, tags: { #e: [...] } }` for replies.
crates/nmp-core/src/substrate/identity.rs:59:    pub tags: Vec<Vec<String>>,
crates/nmp-core/src/planner/plan.rs:108:    pub role_tags: BTreeSet<RoutingSource>,
crates/nmp-core/src/substrate/view.rs:12:    pub tags: Vec<Vec<String>>,
crates/nmp-core/src/planner/interest.rs:28:/// A Nostr tag key (e.g. "e", "p", "t", "a").
crates/nmp-core/src/planner/interest.rs:84:    pub tags: BTreeMap<TagKey, BTreeSet<String>>,
crates/nmp-core/src/planner/compiler/partition.rs:80:        tags: interest.shape.tags.clone(),
crates/nmp-core/src/planner/compiler/partition.rs:201:        .get("p")
crates/nmp-core/src/planner/compiler/mod.rs:130:            let mut role_tags: BTreeSet<RoutingSource> = BTreeSet::new();
crates/nmp-core/src/planner/compiler/mod.rs:136:                        role_tags.insert(source);
crates/nmp-core/src/planner/compiler/plan_id.rs:70:/// Referenced pubkeys = `interest.shape.authors ∪ addresses[*].pubkey ∪ tags["p"][*]`
crates/nmp-core/src/planner/compiler/plan_id.rs:78:        if let Some(p_values) = interest.shape.tags.get("p") {
crates/nmp-core/src/planner/lattice/mod.rs:104:        tags: merged_tags,
crates/nmp-core/src/planner/lattice/mod.rs:205:        let a = InterestShape { tags: tags_a, kinds: [1].into_iter().collect(), ..Default::default() };
crates/nmp-core/src/planner/lattice/mod.rs:206:        let b = InterestShape { tags: tags_b, kinds: [1].into_iter().collect(), ..Default::default() };
crates/nmp-core/src/planner/lattice/mod.rs:222:        let a = InterestShape { tags: tags_a, ..Default::default() };
crates/nmp-core/src/planner/lattice/mod.rs:223:        let b = InterestShape { tags: tags_b, ..Default::default() };

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/lattice/rules.rs | sed -n '1,70p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	//! Individual merge rule implementations for the filter-merge lattice.
     2	//!
     3	//! Each function corresponds to one rule from compiler.md §3.3.
     4	//! All rules are `pub(super)` — only the lattice `merge()` entry point is public.
     5	//!
     6	//! Design: `docs/design/subscription-compilation/compiler.md` §3.3
     7	//! Doctrine: D8 (zero per-event allocs on the hot path after warmup).
     8	
     9	use crate::planner::interest::{InterestLifecycle, InterestShape, NaddrCoord};
    10	
    11	/// Rule 1 — `kinds` merge.
    12	///
    13	/// Mergeable iff `a.kinds == b.kinds` OR one is empty (wildcard absorbs ALL).
    14	///
    15	/// An empty set means "match any kind" (wildcard). When either side is wildcard,
    16	/// the result MUST be wildcard (empty), not the other side's concrete set.
    17	/// Returning the concrete set would NARROW the subscription semantics — a relay
    18	/// receiving `{ kinds: [1, 6] }` would miss kinds 0, 30023, etc. that the
    19	/// wildcard side intended to include.
    20	///
    21	/// `wildcard ∪ {1, 6} = wildcard` — the wildcard absorbs its neighbour.
    22	pub(super) fn rule1_kinds(
    23	    a: &InterestShape,
    24	    b: &InterestShape,
    25	) -> Option<std::collections::BTreeSet<u32>> {
    26	    if a.kinds.is_empty() || b.kinds.is_empty() {
    27	        // At least one side is wildcard — wildcard absorbs, result is wildcard.
    28	        Some(std::collections::BTreeSet::new())
    29	    } else if a.kinds == b.kinds {
    30	        Some(a.kinds.clone())
    31	    } else {
    32	        // Both non-empty but different — refuse (merging would widen kinds)
    33	        None
    34	    }
    35	}
    36	
    37	/// Rule 2 — `tags` merge.
    38	///
    39	/// Mergeable iff both shapes have the same tag key dimensions, AND the union
    40	/// of values per dimension stays under `limit`.
    41	pub(super) fn rule2_tags(
    42	    a: &InterestShape,
    43	    b: &InterestShape,
    44	    limit: usize,
    45	) -> Option<std::collections::BTreeMap<crate::planner::interest::TagKey, std::collections::BTreeSet<String>>> {
    46	    // Keys must be identical (same dimensions)
    47	    if a.tags.keys().ne(b.tags.keys()) {
    48	        return None;
    49	    }
    50	
    51	    let mut merged = std::collections::BTreeMap::new();
    52	    for (key, av) in &a.tags {
    53	        let bv = b.tags.get(key)?;
    54	        let union: std::collections::BTreeSet<String> = av.union(bv).cloned().collect();
    55	        if union.len() > limit {
    56	            return None;
    57	        }
    58	        merged.insert(key.clone(), union);
    59	    }
    60	    Some(merged)
    61	}
    62	
    63	/// Rule 3 — `since` merge.
    64	///
    65	/// Returns `min(a.since, b.since)` iff both are `Some` or both are `None`.
    66	/// Mixed (one bounded, one unbounded) returns `None` (refuse).
    67	pub(super) fn rule3_since(a: &InterestShape, b: &InterestShape) -> Option<Option<u64>> {
    68	    match (a.since, b.since) {
    69	        (None, None) => Some(None),
    70	        (Some(sa), Some(sb)) => Some(Some(sa.min(sb))),

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/lattice/mod.rs | sed -n '130,205p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   130	            kinds: kinds.iter().copied().collect(),
   131	            ..Default::default()
   132	        }
   133	    }
   134	
   135	    // ── Rule 1 — kinds ───────────────────────────────────────────────────────
   136	
   137	    #[test]
   138	    fn rule1_equal_kinds_merge() {
   139	        let a = shape_with_kinds(&[1, 6]);
   140	        let b = shape_with_kinds(&[1, 6]);
   141	        let r = merge(&a, &b, &tailing(), &tailing());
   142	        assert!(matches!(r, MergeOutcome::Merged(ref s) if s.kinds == a.kinds));
   143	    }
   144	
   145	    #[test]
   146	    fn rule1_different_kinds_refuse() {
   147	        let a = shape_with_kinds(&[1]);
   148	        let b = shape_with_kinds(&[6]);
   149	        assert_eq!(merge(&a, &b, &tailing(), &tailing()), MergeOutcome::Refused);
   150	    }
   151	
   152	    #[test]
   153	    fn rule1_wildcard_absorbs_specific() {
   154	        // a is wildcard (empty), b is specific — result MUST be wildcard (empty),
   155	        // NOT b.kinds. Returning b.kinds would narrow the merged subscription,
   156	        // causing the relay to miss kinds that the wildcard side intended to match.
   157	        let a = InterestShape::default(); // kinds = empty (wildcard)
   158	        let b = shape_with_kinds(&[1, 6]);
   159	        let r = merge(&a, &b, &tailing(), &tailing());
   160	        assert!(
   161	            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
   162	            "wildcard ∪ {{1,6}} must be wildcard (empty set), not {{1,6}}"
   163	        );
   164	    }
   165	
   166	    #[test]
   167	    fn wildcard_unions_with_anything_stays_wildcard() {
   168	        // Negative-direction: wildcard merged with ANY concrete set must stay wildcard.
   169	        // This is the correctness test the T30 codex review flagged as missing.
   170	        let wildcard = InterestShape::default(); // kinds = empty
   171	        for concrete_kinds in [
   172	            vec![1u32],
   173	            vec![6],
   174	            vec![1, 6],
   175	            vec![0, 1, 3, 4, 5, 6, 7, 9, 10, 30023],
   176	        ] {
   177	            let concrete = shape_with_kinds(&concrete_kinds);
   178	            let r_ab = merge(&wildcard, &concrete, &tailing(), &tailing());
   179	            let r_ba = merge(&concrete, &wildcard, &tailing(), &tailing());
   180	            assert!(
   181	                matches!(r_ab, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
   182	                "wildcard ∪ {:?} must be wildcard (a=wildcard)", concrete_kinds
   183	            );
   184	            assert!(
   185	                matches!(r_ba, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
   186	                "wildcard ∪ {:?} must be wildcard (b=wildcard)", concrete_kinds
   187	            );
   188	        }
   189	        // wildcard ∪ wildcard = wildcard
   190	        let r = merge(&wildcard, &wildcard, &tailing(), &tailing());
   191	        assert!(
   192	            matches!(r, MergeOutcome::Merged(ref s) if s.kinds.is_empty()),
   193	            "wildcard ∪ wildcard must be wildcard"
   194	        );
   195	    }
   196	
   197	    // ── Rule 2 — tags ────────────────────────────────────────────────────────
   198	
   199	    #[test]
   200	    fn rule2_same_tag_dimensions_merge() {
   201	        let mut tags_a = BTreeMap::new();
   202	        tags_a.insert("t".to_string(), ["bitcoin".to_string()].into_iter().collect::<BTreeSet<_>>());
   203	        let mut tags_b = BTreeMap::new();
   204	        tags_b.insert("t".to_string(), ["nostr".to_string()].into_iter().collect::<BTreeSet<_>>());
   205	        let a = InterestShape { tags: tags_a, kinds: [1].into_iter().collect(), ..Default::default() };

exec
/bin/zsh -lc "git show bc84cfe --unified=0 --no-ext-diff -- . ':(exclude)Cargo.lock' | rg -n '"'^'"\\+.*(TODO|FIXME|unimplemented"'!|todo!|not yet implemented|not_enabled)'"' || true" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
exec
/bin/zsh -lc "git show 7d16b3f --unified=0 --no-ext-diff -- . ':(exclude)Cargo.lock' | rg -n '"'^'"\\+.*(TODO|FIXME|unimplemented"'!|todo!|not yet implemented|not_enabled)'"' || true" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
2448:+        return Err(StoreError::Io("CBOR dump not yet implemented".into()));

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/kernel/ingest.rs | sed -n '130,175p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   130	        outbound.extend(self.maybe_open_thread_hydration());
   131	        outbound
   132	    }
   133	
   134	    pub(super) fn handle_event(&mut self, role: RelayRole, sub_id: &str, value: &Value) {
   135	        let Ok(event) = serde_json::from_value::<NostrEvent>(value.clone()) else {
   136	            self.log(format!("bad EVENT payload on {sub_id}"));
   137	            return;
   138	        };
   139	
   140	        let now = Instant::now();
   141	        {
   142	            let relay = self.relay_mut(role);
   143	            relay.counters.events_rx = relay.counters.events_rx.saturating_add(1);
   144	            relay.last_event_at = Some(now);
   145	        }
   146	        self.events_since_last_update = self.events_since_last_update.saturating_add(1);
   147	        self.last_event_at = Some(now);
   148	        self.first_event_at.get_or_insert(now);
   149	        if let Some(sub) = self.wire_subs.get_mut(sub_id) {
   150	            if sub.state == "opening" {
   151	                sub.state = "live".to_string();
   152	            }
   153	            sub.last_event_at = Some(now);
   154	        }
   155	
   156	        match event.kind {
   157	            0 => self.ingest_profile(event),
   158	            1 | 6 => self.ingest_timeline_event(role, sub_id, event),
   159	            3 => self.ingest_contacts(event),
   160	            10002 => self.ingest_relay_list(event),
   161	            _ => {}
   162	        }
   163	        self.changed_since_emit = true;
   164	    }
   165	
   166	    pub(super) fn ingest_profile(&mut self, event: NostrEvent) {
   167	        let candidate = parse_profile(&event);
   168	        let should_replace = self
   169	            .profiles
   170	            .get(&event.pubkey)
   171	            .map(|current| {
   172	                candidate.created_at > current.created_at
   173	                    || (candidate.created_at == current.created_at
   174	                        && candidate.event_id < current.event_id)
   175	            })

codex
Two T29 problems are emerging: the kernel still returns before the store sees duplicate relays, and the GC claim code uses a `Vec` where the spec requires intra-call de-duping. For T30, the wildcard implementation/test are present, but I’m checking the #p direction-table edge cases against the design text.
exec
/bin/zsh -lc 'cargo test -p nmp-core planner::lattice::tests::wildcard_unions_with_anything_stays_wildcard -- --exact' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 156ms:
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running unittests src/lib.rs (target/debug/deps/nmp_core-c5f45f3d1b1f1c2d)

running 1 test
test planner::lattice::tests::wildcard_unions_with_anything_stays_wildcard ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s

     Running tests/substrate_registry.rs (target/debug/deps/substrate_registry-73cd2d841b9cceb0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s


exec
/bin/zsh -lc 'cargo test -p nmp-testing --test m2_plan_id_stability' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running tests/m2_plan_id_stability.rs (target/debug/deps/m2_plan_id_stability-2264b0d4fa31915d)

running 3 tests
test plan_id_changes_on_indexer_set_version_bump ... ok
test plan_id_unchanged_when_unrelated_mailbox_arrives ... ok
test plan_id_changes_when_referenced_author_mailbox_updates ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


exec
/bin/zsh -lc 'cargo test -p nmp-testing --test m2_subscription_compilation_audit' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 104ms:
exec
/bin/zsh -lc 'cargo test -p nmp-core store::mem -- --nocapture' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.04s
     Running unittests src/lib.rs (target/debug/deps/nmp_core-c5f45f3d1b1f1c2d)

running 8 tests
test store::mem::gc::tests::release_clears_all_pins ... ok
test store::mem::tests::gc_tests::claim_idempotent_reclaim_does_not_count ... ok
test store::mem::gc::tests::claim_over_per_view_ceiling_returns_err ... ok
test store::mem::tests::gc_tests::claim_over_per_view_ceiling_returns_err ... ok
test store::mem::tests::gc_tests::release_clears_all_pins ... ok
test store::mem::gc::tests::claim_idempotent_reclaim_does_not_count ... ok
test store::mem::tests::insert_tests::tombstone_max_merge_takes_newer_deleted_at ... ok
test store::mem::tests::insert_tests::replaceable_dup_id_merges_provenance ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.00s

     Running tests/substrate_registry.rs (target/debug/deps/substrate_registry-73cd2d841b9cceb0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s


exec
/bin/zsh -lc "rg -n \"#p|\\\"p\\\"|read_relays|active_account_read|request_probe|both_relays\" crates/nmp-testing/tests/m2_subscription_compilation_audit.rs crates/nmp-testing/tests/m2_plan_id_stability.rs crates/nmp-core/src/planner -S" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:61:                read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:62:                both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:67:                read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:68:                both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:73:                read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:74:                both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:189:            read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:190:            both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:313:            read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:314:            both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:321:            read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:322:            both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:377:            read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:378:            both_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:417:            read_relays: vec![],
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:418:            both_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:4://! referenced by the interest set (authors, #p tags, address pubkeys), not the
crates/nmp-testing/tests/m2_plan_id_stability.rs:47:/// author set, #p tags, or address pubkeys — MUST NOT change the plan-id.
crates/nmp-testing/tests/m2_plan_id_stability.rs:63:            read_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:64:            both_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:95:            read_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:96:            both_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:128:            read_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:129:            both_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:160:            read_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:161:            both_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:191:            read_relays: vec![],
crates/nmp-testing/tests/m2_plan_id_stability.rs:192:            both_relays: vec![],
crates/nmp-core/src/planner/compiler/mailbox.rs:17:/// Phase 1: only `write_relays` and `both_relays` are consumed (Outbox
crates/nmp-core/src/planner/compiler/mailbox.rs:18:/// direction). Inbox direction (read_relays) is used for `#p` interests.
crates/nmp-core/src/planner/compiler/mailbox.rs:24:    pub read_relays: Vec<RelayUrl>,
crates/nmp-core/src/planner/compiler/mailbox.rs:25:    pub both_relays: Vec<RelayUrl>,
crates/nmp-core/src/planner/compiler/mailbox.rs:31:        self.write_relays.iter().chain(self.both_relays.iter())
crates/nmp-core/src/planner/compiler/mailbox.rs:53:    fn request_probe(&self, _pubkey: &Pubkey) {
crates/nmp-core/src/planner/compiler/plan_id.rs:67:/// any interest's author set, #p tags, or address pubkeys) MUST NOT change
crates/nmp-core/src/planner/compiler/plan_id.rs:70:/// Referenced pubkeys = `interest.shape.authors ∪ addresses[*].pubkey ∪ tags["p"][*]`
crates/nmp-core/src/planner/compiler/plan_id.rs:78:        if let Some(p_values) = interest.shape.tags.get("p") {
crates/nmp-core/src/planner/compiler/plan_id.rs:97:/// set / #p tags / address pubkeys) MUST NOT change the plan-id.
crates/nmp-core/src/planner/compiler/plan_id.rs:142:            let mut read_sorted = mb.read_relays.clone();
crates/nmp-core/src/planner/compiler/plan_id.rs:145:            let mut both_sorted = mb.both_relays.clone();
crates/nmp-core/src/planner/compiler/mod.rs:57:/// | Has `#p` tag values     | Inbox     | tagged pubkey's read relays (post-v1 DMs/notifs) |
crates/nmp-core/src/planner/compiler/mod.rs:66:    active_account_read_relays: &'a [RelayUrl],
crates/nmp-core/src/planner/compiler/mod.rs:72:        Self { mailbox_cache, indexer_relays, active_account_read_relays: &[] }
crates/nmp-core/src/planner/compiler/mod.rs:77:    /// When `active_account_read_relays` is non-empty, no-author interests
crates/nmp-core/src/planner/compiler/mod.rs:80:    pub fn with_active_account_read_relays(
crates/nmp-core/src/planner/compiler/mod.rs:83:        active_account_read_relays: &'a [RelayUrl],
crates/nmp-core/src/planner/compiler/mod.rs:85:        Self { mailbox_cache, indexer_relays, active_account_read_relays }
crates/nmp-core/src/planner/compiler/mod.rs:122:                self.active_account_read_relays,
crates/nmp-core/src/planner/compiler/partition.rs:64:/// - **Case C (#p)**: no authors/addresses, but `#p` tag values → Inbox
crates/nmp-core/src/planner/compiler/partition.rs:66:///   private `#p` interests to non-inbox relays.
crates/nmp-core/src/planner/compiler/partition.rs:68:/// - **Case D (no-author)**: no authors, addresses, or #p → active-account
crates/nmp-core/src/planner/compiler/partition.rs:74:    active_account_read_relays: &[RelayUrl],
crates/nmp-core/src/planner/compiler/partition.rs:190:    // Case C: #p tag values → Inbox (tagged pubkey's read relays).
crates/nmp-core/src/planner/compiler/partition.rs:192:    // #p interests (DMs, notifications) MUST route to the tagged pubkey's READ
crates/nmp-core/src/planner/compiler/partition.rs:196:    // Phase 1 stub: read_relays not yet populated from kind:10002 → fall back
crates/nmp-core/src/planner/compiler/partition.rs:201:        .get("p")
crates/nmp-core/src/planner/compiler/partition.rs:208:                Some(snapshot) if !snapshot.read_relays.is_empty() => {
crates/nmp-core/src/planner/compiler/partition.rs:209:                    for relay in &snapshot.read_relays {
crates/nmp-core/src/planner/compiler/partition.rs:221:                    mailbox_cache.request_probe(tagged_pk);
crates/nmp-core/src/planner/compiler/partition.rs:240:    // Case D: no authors, addresses, or #p → active-account read relays / indexer.
crates/nmp-core/src/planner/compiler/partition.rs:241:    let (fallback_relays, fallback_source) = if !active_account_read_relays.is_empty() {
crates/nmp-core/src/planner/compiler/partition.rs:242:        (active_account_read_relays,
crates/nmp-core/src/planner/interest.rs:28:/// A Nostr tag key (e.g. "e", "p", "t", "a").

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/planner/lattice/mod.rs | sed -n '40,120p' && nl -ba crates/nmp-testing/tests/m2_subscription_compilation_audit.rs | sed -n '80,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
    40	    Refused,
    41	}
    42	
    43	/// Attempt to merge shape `b` into shape `a` on a given relay.
    44	///
    45	/// Returns `Merged(result)` iff all 8 rules pass; `Refused` otherwise.
    46	/// Neither `a` nor `b` is modified on refusal.
    47	///
    48	/// Design: §3.3 Rules 1–8
    49	pub fn merge(
    50	    a: &InterestShape,
    51	    b: &InterestShape,
    52	    lifecycle_a: &InterestLifecycle,
    53	    lifecycle_b: &InterestLifecycle,
    54	) -> MergeOutcome {
    55	    // Rule 6 first — cheapest check, prune early.
    56	    if !rule6_lifecycle(lifecycle_a, lifecycle_b) {
    57	        return MergeOutcome::Refused;
    58	    }
    59	
    60	    // Rule 1 — kinds
    61	    let merged_kinds = match rule1_kinds(a, b) {
    62	        Some(k) => k,
    63	        None => return MergeOutcome::Refused,
    64	    };
    65	
    66	    // Rule 2 — tag dimensions
    67	    let merged_tags = match rule2_tags(a, b, DEFAULT_VALUE_LIMIT) {
    68	        Some(t) => t,
    69	        None => return MergeOutcome::Refused,
    70	    };
    71	
    72	    // Rule 3 — since
    73	    let merged_since = match rule3_since(a, b) {
    74	        Some(s) => s,
    75	        None => return MergeOutcome::Refused,
    76	    };
    77	
    78	    // Rule 4 — until
    79	    let merged_until = match rule4_until(a, b) {
    80	        Some(u) => u,
    81	        None => return MergeOutcome::Refused,
    82	    };
    83	
    84	    // Rule 5 — limit
    85	    if !rule5_limit(a, b) {
    86	        return MergeOutcome::Refused;
    87	    }
    88	
    89	    // Rule 7 — event_ids union
    90	    let merged_event_ids = match rule7_event_ids(a, b, DEFAULT_VALUE_LIMIT) {
    91	        Some(ids) => ids,
    92	        None => return MergeOutcome::Refused,
    93	    };
    94	
    95	    // Rule 8 — addresses union (requires prior rules to have passed)
    96	    let merged_addresses = match rule8_addresses(a, b, DEFAULT_VALUE_LIMIT) {
    97	        Some(addrs) => addrs,
    98	        None => return MergeOutcome::Refused,
    99	    };
   100	
   101	    MergeOutcome::Merged(InterestShape {
   102	        authors: a.authors.union(&b.authors).cloned().collect(),
   103	        kinds: merged_kinds,
   104	        tags: merged_tags,
   105	        since: merged_since,
   106	        until: merged_until,
   107	        limit: None, // Rule 5 guarantees both are None
   108	        event_ids: merged_event_ids,
   109	        addresses: merged_addresses,
   110	    })
   111	}
   112	
   113	// ─── Tests ───────────────────────────────────────────────────────────────────
   114	
   115	#[cfg(test)]
   116	mod tests {
   117	    use super::*;
   118	    use crate::planner::interest::{InterestLifecycle, InterestShape, NaddrCoord};
   119	    use std::collections::{BTreeMap, BTreeSet};
   120	
    80	}
    81	
    82	fn interest_id(n: u64) -> InterestId {
    83	    InterestId(n)
    84	}
    85	
    86	// ─── Assertion 2 — per-author wire fan-out ────────────────────────────────────
    87	
    88	/// "For a timeline of N authors, the compiled plan opens REQs only against
    89	/// the union of those authors' write relays (de-duplicated). Each relay carries
    90	/// exactly one merged sub-shape."
    91	///
    92	/// Design: `docs/design/subscription-compilation/tests.md` §9.2 Assertion 2.
    93	#[test]
    94	fn timeline_compiles_to_per_relay_union() {
    95	    // Design spec §9.2 Assertion 2 states 1000 authors as the boundary.
    96	    let authors = make_authors_with_overlapping_mailboxes(1000);
    97	
    98	    let mut cache = InMemoryMailboxCache::new();
    99	    for (pk, mb) in &authors {
   100	        cache.put(pk.clone(), mb.clone());
   101	    }
   102	
   103	    let indexer = vec![relay("wss://purplepag.es")];
   104	    let compiler = SubscriptionCompiler::new(&cache, &indexer);
   105	
   106	    let author_set: BTreeSet<String> = authors.iter().map(|(pk, _)| pk.clone()).collect();
   107	    let interest = LogicalInterest {
   108	        id: interest_id(1),
   109	        scope: InterestScope::ActiveAccount,
   110	        shape: InterestShape {
   111	            authors: author_set.clone(),
   112	            kinds: [1u32, 6u32].into_iter().collect(),
   113	            ..Default::default()
   114	        },
   115	        hints: vec![],
   116	        lifecycle: InterestLifecycle::Tailing,
   117	    };
   118	
   119	    let plan = compiler.compile(&[interest]).expect("compile");
   120	

exec
/bin/zsh -lc "rg -n \"\\.compile\\(|compile_with_context|SubscriptionCompiler::\" crates -S" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
crates/nmp-testing/tests/m2_plan_id_stability.rs:84:        let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_plan_id_stability.rs:86:            .compile_with_context(std::slice::from_ref(&interest), &ctx)
crates/nmp-testing/tests/m2_plan_id_stability.rs:101:        let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_plan_id_stability.rs:103:            .compile_with_context(std::slice::from_ref(&interest), &ctx)
crates/nmp-testing/tests/m2_plan_id_stability.rs:149:        let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_plan_id_stability.rs:151:            .compile_with_context(std::slice::from_ref(&interest), &ctx)
crates/nmp-testing/tests/m2_plan_id_stability.rs:166:        let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_plan_id_stability.rs:168:            .compile_with_context(std::slice::from_ref(&interest), &ctx)
crates/nmp-testing/tests/m2_plan_id_stability.rs:197:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_plan_id_stability.rs:215:        .compile_with_context(std::slice::from_ref(&interest), &ctx_v0)
crates/nmp-testing/tests/m2_plan_id_stability.rs:218:        .compile_with_context(std::slice::from_ref(&interest), &ctx_v1)
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:104:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:119:    let plan = compiler.compile(&[interest]).expect("compile");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:157:    let plan2 = compiler.compile(&[LogicalInterest {
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:195:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:217:        .compile(&[make_interest(10), make_interest(11)])
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:266:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:280:    let plan = compiler.compile(&[interest]).expect("compile");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:327:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:349:    let plan = compiler.compile(&[interest]).expect("compile");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:383:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:397:    let plan_a = compiler.compile(&interests).expect("compile a");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:398:    let plan_b = compiler.compile(&interests).expect("compile b");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:423:    let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:448:    let plan_one = compiler.compile(std::slice::from_ref(&interest_a)).expect("compile one");
crates/nmp-testing/tests/m2_subscription_compilation_audit.rs:450:        .compile(&[interest_a, interest_b])
crates/nmp-core/src/planner/compiler/mod.rs:90:    /// Equivalent to `compile_with_context(interests, &CompileContext::default())`.
crates/nmp-core/src/planner/compiler/mod.rs:91:    /// Use `compile_with_context` when tracking policy version counters.
crates/nmp-core/src/planner/compiler/mod.rs:103:        self.compile_with_context(interests, &CompileContext::default())
crates/nmp-core/src/planner/compiler/mod.rs:110:    pub fn compile_with_context(
crates/nmp-core/src/planner/mod.rs:23://! let compiler = SubscriptionCompiler::new(&cache, &indexer);
crates/nmp-core/src/planner/mod.rs:24://! let plan = compiler.compile(&[interest])?;

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/store/lmdb.rs | sed -n '130,210p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   130	    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
   131	        Err(Self::not_enabled())
   132	    }
   133	
   134	    fn tombstones_for(&self, _target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
   135	        Err(Self::not_enabled())
   136	    }
   137	
   138	    fn list_tombstones<'a>(
   139	        &'a self,
   140	    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
   141	    {
   142	        Err(Self::not_enabled())
   143	    }
   144	
   145	    fn provenance_for(&self, _id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
   146	        Err(Self::not_enabled())
   147	    }
   148	
   149	    fn insert(
   150	        &self,
   151	        _event: VerifiedEvent,
   152	        _source: &RelayUrl,
   153	        _received_at_ms: u64,
   154	    ) -> Result<InsertOutcome, StoreError> {
   155	        Err(Self::not_enabled())
   156	    }
   157	
   158	    fn delete_by_filter(&self, _filter: DeleteFilter) -> Result<usize, StoreError> {
   159	        Err(Self::not_enabled())
   160	    }
   161	
   162	    fn read_watermark(&self, _key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
   163	        Err(Self::not_enabled())
   164	    }
   165	
   166	    fn write_watermark(&self, _row: WatermarkRow) -> Result<(), StoreError> {
   167	        Err(Self::not_enabled())
   168	    }
   169	
   170	    fn coverage(&self, _key: &WatermarkKey) -> Result<Coverage, StoreError> {
   171	        Err(Self::not_enabled())
   172	    }
   173	
   174	    fn list_watermarks_for_relay<'a>(
   175	        &'a self,
   176	        _relay_url: &str,
   177	    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
   178	    {
   179	        Err(Self::not_enabled())
   180	    }
   181	
   182	    fn register_view_cover(
   183	        &self,
   184	        _claimer: ClaimerId,
   185	        _cover_budget: usize,
   186	    ) -> Result<(), StoreError> {
   187	        Err(Self::not_enabled())
   188	    }
   189	
   190	    fn claim(&self, _claimer: ClaimerId, _ids: &[EventId]) -> Result<(), StoreError> {
   191	        Err(Self::not_enabled())
   192	    }
   193	
   194	    fn release(&self, _claimer: ClaimerId) -> Result<(), StoreError> {
   195	        Err(Self::not_enabled())
   196	    }
   197	
   198	    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
   199	        Err(Self::not_enabled())
   200	    }
   201	
   202	    fn gc_step(&self, _budget: GcBudget) -> Result<GcReport, StoreError> {
   203	        Err(Self::not_enabled())
   204	    }
   205	
   206	    fn domain_open(&self, _namespace: &'static str) -> Result<DomainHandle, StoreError> {
   207	        Err(Self::not_enabled())
   208	    }
   209	
   210	    fn run_migrations(

codex
**Findings**

T29 is not fully clean.

- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:235): duplicate relay deliveries return before `self.store.insert()`, so EventStore provenance never sees later relay sources. Fix: verify/insert first, then update the read-cache/`relay_count` based on `InsertOutcome::Duplicate`.
- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:156): only kind `1 | 6` events go through EventStore; kind `0`, `3`, `10002` still bypass it. If “D4 wire ingest through EventStore” meant all relay EVENTs, this is partial.
- [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:35): `claim()` collects `new_ids` into a `Vec`, so duplicate IDs in the same call count multiple times despite the BTreeSet/idempotency doctrine. Also [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:52) counts global pins as sum of per-claimer sets, not unique pinned event IDs. Fix: collect intra-call `new_ids` into a `BTreeSet` and compute global ceiling from the union.
- [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:79): `release()` leaves `claim_budgets` behind, contrary to the GC design’s bounded view-budget map. Fix: remove the claimer budget on release.
- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:263): invalid short IDs can panic on `&event.id[..16]`. Fix with a safe prefix helper.

T30 is also not fully clean.

- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:88): if an interest has both `authors` and `#p`, Case A returns before Case C, so inbox routing is never emitted. Fix: split `#p` routing in addition to author/address routing.
- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:208): `#p` inbox routing uses only `read_relays`, but the design says `read_relays ∪ both_relays`. Fix: include/dedup `both_relays`.
- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:220): unknown `#p` mailboxes fall back to indexers while the same block claims a structural ban on non-inbox routes. Fix: request probe and fail closed/no plan for privacy-sensitive `#p`, or weaken the claim if indexer fallback is intentional.

**Checks**

- No changed hand-authored file exceeds 500 LOC.
- Soft-limit >300 remains:
  - T29: `kernel/ingest.rs` 427, `kernel/mod.rs` 417, `store/mem/query.rs` 394, `store/mem/insert.rs` 357.
  - T30: `m2_subscription_compilation_audit.rs` 460, `planner/lattice/mod.rs` 389.
- No new `TODO`/`FIXME` markers found in changed files. T29 only reintroduces the pre-existing `CBOR dump not yet implemented` string via the file split.
- T29 firehose evidence exists: [1779068929-live.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/firehose-bench/1779068929-live.md:7) and [1779070804-live.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/firehose-bench/1779070804-live.md:7) both say overall passed.
- T30 Rule 1 wildcard fix is present and tested at [lattice/mod.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/lattice/mod.rs:153) and [lattice/mod.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/lattice/mod.rs:167).

I ran the targeted T30/T29 tests; they pass, but they do not cover the concerns above.
tokens used
258,485
**Findings**

T29 is not fully clean.

- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:235): duplicate relay deliveries return before `self.store.insert()`, so EventStore provenance never sees later relay sources. Fix: verify/insert first, then update the read-cache/`relay_count` based on `InsertOutcome::Duplicate`.
- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:156): only kind `1 | 6` events go through EventStore; kind `0`, `3`, `10002` still bypass it. If “D4 wire ingest through EventStore” meant all relay EVENTs, this is partial.
- [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:35): `claim()` collects `new_ids` into a `Vec`, so duplicate IDs in the same call count multiple times despite the BTreeSet/idempotency doctrine. Also [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:52) counts global pins as sum of per-claimer sets, not unique pinned event IDs. Fix: collect intra-call `new_ids` into a `BTreeSet` and compute global ceiling from the union.
- [gc.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/store/mem/gc.rs:79): `release()` leaves `claim_budgets` behind, contrary to the GC design’s bounded view-budget map. Fix: remove the claimer budget on release.
- [ingest.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/kernel/ingest.rs:263): invalid short IDs can panic on `&event.id[..16]`. Fix with a safe prefix helper.

T30 is also not fully clean.

- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:88): if an interest has both `authors` and `#p`, Case A returns before Case C, so inbox routing is never emitted. Fix: split `#p` routing in addition to author/address routing.
- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:208): `#p` inbox routing uses only `read_relays`, but the design says `read_relays ∪ both_relays`. Fix: include/dedup `both_relays`.
- [partition.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/compiler/partition.rs:220): unknown `#p` mailboxes fall back to indexers while the same block claims a structural ban on non-inbox routes. Fix: request probe and fail closed/no plan for privacy-sensitive `#p`, or weaken the claim if indexer fallback is intentional.

**Checks**

- No changed hand-authored file exceeds 500 LOC.
- Soft-limit >300 remains:
  - T29: `kernel/ingest.rs` 427, `kernel/mod.rs` 417, `store/mem/query.rs` 394, `store/mem/insert.rs` 357.
  - T30: `m2_subscription_compilation_audit.rs` 460, `planner/lattice/mod.rs` 389.
- No new `TODO`/`FIXME` markers found in changed files. T29 only reintroduces the pre-existing `CBOR dump not yet implemented` string via the file split.
- T29 firehose evidence exists: [1779068929-live.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/firehose-bench/1779068929-live.md:7) and [1779070804-live.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/firehose-bench/1779070804-live.md:7) both say overall passed.
- T30 Rule 1 wildcard fix is present and tested at [lattice/mod.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/lattice/mod.rs:153) and [lattice/mod.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-core/src/planner/lattice/mod.rs:167).

I ran the targeted T30/T29 tests; they pass, but they do not cover the concerns above.
