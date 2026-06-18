# Codex review — #1493/p1 relay_diagnostics presentation-formatting removal

PR: #1577 (`fix/1493-p1-relay-diagnostics`)
Reviewer: `codex exec` (gpt-5.5, high reasoning) over the full hand-written diff.
Date: 2026-06-19

## Scope reviewed

Removal of pre-formatted display-string fields from the kernel `relay_diagnostics`
projection (`RelayDiagnosticsRow` / `RelayDiagnosticsWireSub`) and its typed
FlatBuffers codec, emitting RAW values instead (raw `role`/`connection`/`auth`
tokens, `bytes_rx`/`bytes_tx: u64`, `total_events_rx: u64`, `discovery_kinds:
Vec<u64>`, `consumer_count: u32`, `events_rx: u64`, raw `state`). The `*_tone`
semantic-hue tokens are deliberately KEPT as raw tokens. Shells (iOS Swift,
Android Kotlin, chirp-tui Rust, chirp-desktop Rust) derive display strings at
render time.

Codex verdict overall: "I did not find display formatting left in the Rust relay
diagnostics projection builder or the Rust typed FB encoder/decoder for the
migrated row/sub fields; the remaining issues are shell parity, unsigned data
fidelity, stale fixtures, and FlatBuffers evolution safety."

## Findings and dispositions

### BLOCKER — iOS KRDG test fixture builds removed FB fields — FIXED
`ios/Chirp/ChirpTests/TypedDiagnosticsLifecycleDecoderTests.swift` still built the
old FlatBuffers fields (`shortWireId`, `stateLabel`, `consumerCountLabel`,
`eventsRxDisplay`, `shortUrl`, `roleLabel`, `connectionLabel`, `authLabel`,
`totalEventsDisplay`, `bytesRxDisplay`) against the regenerated binding, which
would fail to compile in the Chirp iOS CI job. Rewrote both `buildRelayDiagnostics`
helpers to the new raw factory signature (raw `role`/`connection`/`auth` strings,
`bytesRx`/`bytesTx` ulong, `consumerCount` uint, `eventsRx` ulong, `state` string,
`discoveryKinds` vector) and updated the assertions to check the raw decoded
fields plus the shell-side computed display vars. Swift compiles cleanly (link
fails only on the missing Rust static lib, which CI builds first).

### HIGH — schema removes/reorders fields in-place with SCHEMA_VERSION still 1 — REJECTED (by project policy)
Codex flagged that positional FlatBuffers ordinals changed without a version bump,
risking mixed old/new readers decoding wrong slots. This is intentional and
correct for this repo: there is a single producer and all consumers live in the
same monorepo, regenerated in this same PR; downstream apps pin NMP by git rev.
The repo doctrine ("No compat aliases — ever"; coordinated hard-break + upgrade
all consumers in one PR) forbids keeping deprecated ordinals or shim fields. The
buffer carries the `KRDG` file identifier and the decoder fails closed on a
malformed/mismatched buffer. No stale reader can exist post-merge.

### HIGH — Android maps `ulong`/`uint` to signed `Long`/`Int` — REJECTED (pre-existing convention, no practical loss)
The Android typed decoder maps every `ulong` via `.toLong()` and every `uint` via
`.toInt()`. This is the established, pre-existing convention for ALL kernel
projections (e.g. `total_events_rx` was already a `ulong` mapped `.toLong()`
before this PR; all `*_ms` timestamps likewise), and matches the signed accessor
types flatc emits for the Kotlin runtime. The values carried (byte counts, event
counts, kind numbers 0..=19999, sub counts) are orders of magnitude below
`Long.MAX`/`Int.MAX`, so no corruption is reachable. Diverging here would make
relay_diagnostics inconsistent with every other Android projection decoder for no
practical gain; out of scope for this lane.

### HIGH — discovery-kind display not parity-equivalent (iOS vs TUI; Android absent) — FIXED (iOS/TUI), N/A (Android)
iOS rendered empty as `""`, `10002` as `"relay list (10002)"` (space), `10003`
specially, unknowns as `"kind:N"`; the TUI/kernel render empty as `"none"`,
`10002` as `"relay-list (10002)"` (hyphen), unknowns as `"list (N)"`. Aligned the
iOS `discoveryKindsLabel` + `kindName` to the kernel's original output exactly
(`"none"`, `profile/follows/relay-list/list`, `"label (kind)"`), so iOS, TUI, and
the former kernel string now match byte-for-byte. Updated the iOS fixture
assertion to `"profile (0), relay-list (10002)"`. Android does NOT render a
discovery-kinds label today and did not before this PR (it never decoded
`discovery_kinds_label`); the raw `discoveryKinds: List<Long>` is now carried for
a future Android UI. Adding an Android discovery UI is net-new feature work
outside this refactor.

### MEDIUM — byte formatting diverges (Android KiB/MiB vs TUI KB/MB vs iOS ByteCountFormatter) — FIXED
The former kernel `format_bytes` emitted `B`/`KB`/`MB` on a 1024 divisor. The
initial port made Android emit `KiB/MiB/GiB` and iOS use `ByteCountFormatter`
(locale-dependent). Both were real parity regressions vs the prior rendered text.
Rewrote Android `formatBytes` and iOS `formatBytes` to mirror the kernel exactly
(`{n} B` / `{kb:.1} KB` / `{mb:.1} MB`, 1024 divisor), matching the TUI. Also
aligned `compact_count`/`compactCount` across iOS + Android + TUI to drop the
decimal on whole magnitudes (`1K`, not `1.0K`) as the kernel did.

### LOW — Android tests only assert `bytesRxDisplay != null` — FIXED
Strengthened the Android and iOS fixtures to assert the exact rendered byte label
(`"4.0 KB"` Android / `"12.0 KB"` iOS) so the `KB`-vs-`KiB` parity is now
golden-locked per platform.

### MEDIUM — TUI connection classification uses string buckets, not tone — ACKNOWLEDGED (pre-existing, unchanged behavior)
TUI dot/zero-count gating string-matches connection buckets rather than reading
`connection_tone`. The raw `connection` token is the same string the kernel
always emitted (just no longer title-cased), and the TUI's lowercasing match
preserves the prior behavior exactly. Switching the TUI to `connection_tone`
gating is a reasonable cleanup but is a behavior-neutral refactor of pre-existing
TUI logic, out of scope for the presentation-formatting removal.

## Net result

All compile-blocking and real parity regressions identified by the review are
fixed; the two rejected HIGH items are deliberate project-policy / pre-existing
convention decisions documented above. The Rust projection builder and typed FB
codec were confirmed clean of residual display formatting.
