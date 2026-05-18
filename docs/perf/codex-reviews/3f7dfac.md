# Codex review — `3f7dfac` design(content-rendering)

**Reviewed merge:** `3f7dfac9d647ac20a5f88d4b3288c89878b53cf0` — design(content-rendering): should NMP provide a UI layer for Nostr content + recommended shape
**Prior tip:** `79d5f99`
**Review scope:** docs-only (single file `docs/design/content-rendering.md`, 186 LOC).

## Verdict

**Clean — citation fixes only, no design-class follow-ups.**

## Fixes applied in place

Single commit by codex: **`3d95e7a fix(codex): correct content-rendering citations`** (pushed to master).

Five citation corrections, all mechanical:

1. §5 (recursion guard footnote): `event-rendering-flow.md:254-306` was a path inside the NDK-svelte source tree, not a path in the NMP repo. Redirected to `docs/research/content-rendering/ndk-svelte-registry.md:135` (the research file's own recursion-guard finding).
2. §7 trace step 5: bare `NostrEntityCards.swift:55-60` → fully-qualified `ios/NmpHighlighter/Sources/NmpHighlighter/Core/RichText/NostrEntityCards.swift:55-60`.
3. §9 Phase 1: bare `NostrRichText.swift:115-181` → fully-qualified `ios/NmpHighlighter/Sources/NmpHighlighter/Core/RichText/NostrRichText.swift:115-181`.
4. §12 PD-015: same NDK-svelte-internal path redirection as §5.

Codex verified: cited file paths exist, line-number cites land on the claimed content, D0–D8 references match `docs/product-spec/doctrine.md`, markdown links resolve, LOC budget (186 < 300 soft) holds.

## Verified, no fix needed

- File path cites (9 referenced docs / code files) all exist on master at commit time.
- D0–D8 references (8 doctrine cites in §1 + §10) match `docs/product-spec/doctrine.md`.
- Markdown table syntax + cross-doc relative links resolve.
- AGENTS.md file-size budget: 186 LOC ≪ 300 soft.

## Report-class items

None.

## Stale-but-harmless notes

- §1 / §2 / §7 reference `nmp-nip21` as "in flight; sibling worktree." Between submission and review, `65e6812 core(nip19+nip21)` LANDED on master with the full bech32-TLV parser + URI scheme + proptest round-trips. The "in flight" hedge is now stale but doesn't change the design — `nmp_nip21::parse_nostr_uri` exists; the design's call into it is correct. A future follow-up doc-pass can drop the hedging language; not worth a fix-in-place round of its own.

## Tail

- Latest master tip after review: `9f3ab4e perf(pd): resolve PDs 007-010 autonomously per kind-wrappers designer recommendations`.
- Content-rendering design + codex correction are both included.
