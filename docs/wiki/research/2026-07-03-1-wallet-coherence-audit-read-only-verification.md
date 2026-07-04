---
type: research-record
date: 2026-07-03
session: 91a86fdf-624c-446e-9b38-0fb02085121f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/91a86fdf-624c-446e-9b38-0fb02085121f.jsonl
source_lines: 1254-1316
agent_attribution: wallet-audit
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Wallet coherence audit: read-only verification of design↔epic↔crates↔nips.md alignment, verdict COHERENT-WITH-FINDINGS, 4 ranked findings (F2 PaymentPort ownership drift, F1 double-exclusive claim, F3/F4 stale docs) plus cleared non-findings and ordered remaining-work plan"
captured_at: 2026-07-03T10:48:22Z
---

Wallet coherence audit: read-only verification of design↔epic↔crates↔nips.md alignment, verdict COHERENT-WITH-FINDINGS, 4 ranked findings (F2 PaymentPort ownership drift, F1 double-exclusive claim, F3/F4 stale docs) plus cleared non-findings and ordered remaining-work plan

---

<teammate-message teammate_id="wallet-audit" color="purple" summary="NIP-60/61 wallet coherence audit complete">
NIP-60/61/wallet coherence audit — READ-ONLY, no files changed.

IMPORTANT METHOD NOTE: my local checkout was stale (HEAD `8c479c972`, no `nmp-wallet`). The campaign PRs are all merged on `origin/master` (HEAD `0d3d053ef`). #2877 is MERGED (not "may still be landing"). I fetched and audited against `origin/master` via a throwaway worktree (removed). All file:line refs below are as of `origin/master`.

## 1. COHERENCE VERDICT: COHERENT-WITH-FINDINGS

The spine reconciles well: nmp-core is clean, no false surfaces, journal/capability/projection shapes match the design, relay-authority fix landed. Four findings, ranked.

**F2 (HIGH — the one that matters) — nmp-wallet reintroduces PaymentPort-adapter *ownership* the design doc explicitly retracted, plus a duplicate type name.**
- Design doc `docs/architecture/nip60-nip61-wallet-design.md:73` ("`nmp-wallet` does not own or reassign `PaymentPort` itself") and `:104-107` ("This design does not reassign `PaymentPort` ownership away from `nmp-nip47`; `nmp-wallet` only selects which backend that port routes to"), plus crate-boundaries §8 `docs/architecture/crate-boundaries.md:301-302` ("`nmp-nip47` owns ... the `PaymentPort` implementation (`WalletPaymentPort`)").
- But shipped nmp-wallet contradicts that: `crates/nmp-wallet/src/payment_port.rs:28-49` defines its OWN `pub struct WalletPaymentPort` implementing `nmp_core::substrate::PaymentPort` — same name, same trait as `crates/nmp-nip47/src/payment_port.rs:24-54`. And `crates/nmp-wallet/src/lib.rs:6` claims the crate owns "the payment-port adapter NIP-57 uses"; `crates/nmp-wallet/src/ownership.rs:4` + `:107` claim it owns "the PaymentPort adapter."
- Verdict on your Q1: they are two *genuinely distinct types* (nip47's wraps a `WalletRuntimeHandle` → emits `WalletPayInvoiceCommand`; wallet's wraps an abstract `WalletPaymentCommandFactory` → delegates). So not literal code duplication, and no runtime double-wire *today* because nmp-wallet is unconsumed. BUT: (a) identical name + identical trait is a real confusion hazard, and (b) the lib.rs/ownership prose is a **silent port-ownership reassignment in metadata** — exactly the framing #2869 corrected in the design doc, which the crate's own descriptor then re-asserted. The design's stated selection mechanism is "supply the selected backend's `Arc<dyn PaymentPort>`" via `nmp_nip57::Config::with_payment_port` (`design:82-83,115`) — that needs nmp-nip47's port passed through; it does NOT require nmp-wallet to define a second `PaymentPort` impl at all.
- Fix: either delete nmp-wallet's `WalletPaymentPort`/`WalletPaymentCommandFactory` (selection = pass nip47's `Arc<dyn PaymentPort>` through), or if a routing indirection is truly wanted, rename it (e.g. `WalletBackendPaymentRouter`) so it can't be mistaken for the adapter, and reword `lib.rs:6` + `ownership.rs:4,107` to "selects the PaymentPort backend," never "owns the PaymentPort adapter."

**F1 (MEDIUM) — Ownership double-exclusive on `action/nmp.wallet.pay_invoice`, invisible to the gate.**
- `crates/nmp-nip47/src/ownership.rs:6-18`: claim `nip47.wallet_runtime`, `claim_type:"mechanism"`, `exclusive:true`, scope `{action, nmp.wallet.pay_invoice}`.
- `crates/nmp-wallet/src/ownership.rs:58-70`: claim `action.nmp.wallet.pay_invoice`, `claim_type:"namespace"`, `exclusive:true`, SAME scope `{action, nmp.wallet.pay_invoice}`.
- The linker collision symbol (`crates/nmp-ownership/src/macros.rs:15`) keys on `claim_type + scope_kind + scope_value + context`. Because one says "mechanism" and the other "namespace", the exported symbols differ (`__nmp_own__mechanism__action__nmp.wallet.pay_invoice__` vs `__nmp_own__namespace__action__…`) and the gate never fires. Two crates thus both hold an unchallenged EXCLUSIVE claim on one action namespace — the exact double-book the gate exists to prevent. (Note: nmp-wallet also exclusively claims connect/disconnect, which nip47 does NOT re-claim; the collision is specific to pay_invoice.)
- Fix: nmp-nip47 should not claim `exclusive:true` on the `action` scope whose namespace it no longer owns — rescope its claim to a runtime/mechanism scope (`{namespace, nmp-nip47}`, as `nmp-nwc` does at `crates/nmp-nwc/src/ownership.rs`), leaving nmp-wallet the sole exclusive owner.

**F3 (LOW-MED) — `docs/nips.md:54` NIP-60/61 row is stale.** Still says "future `nmp-wallet` composition crate **not yet added**" and "No product/UI surface exists yet", and lists #2870 as open follow-ups. As of #2876, nmp-wallet exists (Phase-1 journal spine); #2870 is closed (fixed by #2874). Update the row.

**F4 (LOW) — Finding-G remnant in the design doc.** #2877 substantively closed finding G: Product Surface `design:185-207` now reads "no compatibility aliases", and the shipped test is literally `no_action_namespace_is_duplicated_as_a_compatibility_alias` (`nmp-wallet/src/lib.rs:88`). But the Activation-requires gate list at `design:474` still reads "…journal transitions, projection bounds, and **compatibility aliases**" — a leftover naming a concept the doc elsewhere says doesn't exist. Reword to "action-name uniqueness (no compat aliases)." So: finding G is closed everywhere it's load-bearing; one cosmetic remnant remains.

### Cleared / verified non-findings (don't re-litigate)
- **nmp-core clean**: zero cashu/nutzap/nwc production nouns. The only hits are test files (`nwc-deadbeef` sub name; one "NWC" regression comment) and "mint(ed)" = minting correlation ids/signatures. ✔
- **No false/dead surfaces**: nmp-wallet and nmp-nip60 have zero `todo!`/`unimplemented!`/`Unsupported`/stub. The old `pay_invoice` `Unsupported` stub was deleted in #2866. `WalletIntent` variants (backend.rs) are just enum arms — no ActionModule registers them, so nothing user-facing fails. ✔
- **nmp-wallet genuinely unconsumed/parked**: no Cargo.toml depends on it, no `nmp_wallet` code refs outside the crate; `release/nmp-release.toml:352` classifies it `[[private_packages]]`, excluded from the v1 train. It advertises nothing it can't do. ✔
- **capability.rs** flags exactly match the epic's 8-flag list (pay_bolt11, create_cashu_wallet, publish_nutzap_info, send_nutzap, redeem_nutzap, deposit_cashu, melt_cashu, observe_nutzap_receipts). ✔
- **Projection bounded**: `MAX_WALLET_PROJECTION_ROWS=100`, keep-last truncation (`projection.rs:117`). ✔
- **No Trellis vocabulary** anywhere in nmp-wallet (ADR-0075 clean). ✔
- **Relay authority reconciles**: design + nip60 reality both make kind:10019/NIP-65 authoritative and `legacy_relay_hint` decode-only; #2874 stopped `build_wallet_event` emitting relay tags and made `publish_nutzap_info` take an explicit `relays` param. ✔
- **nmp-nip60 ownership descriptor** fixed per #2870 finding 2 (now "Cashu backend adapter for the `nmp-wallet::WalletBackend` seam"); notes correctly defer journal/seam/selection to nmp-wallet. ✔ nmp-nwc ownership clean, no collision. ✔

## 2. ORDERED REMAINING-WORK LIST (Phase-1 "smallest complete loop")

What exists on master: pure types only — `WalletBackend` trait (no impls), capability flags, the journal saga/fact/ledger/trail (tested), the bounded `"wallet"` projection shape, action-name constants, and the (to-be-reworked) payment_port. NONE of it is wired: no ActionModule, no projection registration, no read interests/observers, no actor/runtime, no backend impls, no mint HTTP. The "smallest loop" spans epic Phases 1–4. Items below; [P]=parallelizable, [S]=sequential, with coupling flags.

- **W1 — Mint-HTTP capability lane** [P, foundational]. Rust-owned worker + browser-fetch-as-raw-capability-result; Rust owns construction/validation/retry/terminal status. Crates: `nmp-core` substrate host-op/capability seam + `nmp-nip60` (`cashu::client` exists, already `native`-gated). Coupling: touches nmp-core substrate. No deps.
- **W3 — NWC backend adapter** (`impl WalletBackend`, `pay_bolt11`) over nmp-nip47 [P]. Crate: `nmp-wallet`. No deps beyond existing nip47. Appends to `lib.rs`.
- **W2 — Cashu backend adapter** (`impl WalletBackend`) over nmp-nip60 [S after W1]. Crate: `nmp-wallet` (dep direction wallet→nip60; note the ownership.rs prose says nip60 "owns the adapter for the seam" but the `impl nmp_wallet::WalletBackend` must physically live in nmp-wallet — worth a one-line ownership clarification). Deps: W1, nip60 token/nutzap/deposit codecs (exist). Appends to `lib.rs`.
- **W4 — Backend selection policy + `select_backend` wiring** [S after W2/W3]. Crate: `nmp-wallet`. **Shared-file coupling point** (backend registry / lib.rs).
- **W5 — Action modules** for all 11 namespaces (connect/disconnect/pay_invoice delegate to nip47; cashu.*; nutzap.*) [S after W4]. Crate: `nmp-wallet` (new `register.rs`). Couples `lib.rs`+`register.rs`.
- **W6 — Projection registration + read interests + observers** [S, with W5]. Register the `"wallet"` typed snapshot; read interests for kind:9321 (`#p`,`#u`), kind:10019, token/history; observer for nutzap receipts. Crate: `nmp-wallet`. Couples `register.rs`.
- **W7 — Actor/runtime wiring** [S, near-last]. The wallet actor that owns backend state, drives the journal saga (exists), consumes `KernelEvent`s, threads `MintResult`. Crate: `nmp-wallet`. Deps: W2–W6. Heaviest coupling.
- **W13 — Publish kind:10019** (`nutzap.publish_info`) using the resolved relay set (kind:10019/NIP-65, NOT legacy_relay_hint; nip60 `publish_nutzap_info` already takes explicit `relays`) [S after W7]. Crate: `nmp-wallet` action + nip60 codec.
- **W8 — Nutzap send** [P after W7]. Resolve recipient kind:10019 via kernel reads; only recipient-listed mints + exact `u`; publish to listed relays; fail-closed on no trusted mint / no P2PK / no reachable relay. Crate: `nmp-wallet` + nip60 `nutzap_send` (exists).
- **W9 — Nutzap receive/redeem** [P after W7, parallel with W8]. Subscribe kinds:[9321]+`#p`+`#u` limited to accepted mints; verify p-tag/mint/P2PK/DLEQ before counting; swap into wallet-owned proofs before marking redeemed; publish kind:7376; never republish received proofs; unverifiable = rejected, never counted. Crate: `nmp-wallet` + nip60 `nutzap_receive`/DLEQ (exist).
- **W10 — Restart reconciliation wiring** [S after W7]. Connect the journal's `Unknown→PublishPending→Settled` reconcile path (logic + tests exist) to real backend/mint probes at startup. Crate: `nmp-wallet`.
- **W11 — NIP-57 PaymentPort backend wiring** [P after W3]. Supply the selected backend's `Arc<dyn PaymentPort>` via `nmp_nip57::Config::with_payment_port`. **Resolve F2 here** (pass nip47's port; don't ship a second `WalletPaymentPort`). Crate: `nmp-wallet` composition.
- **W12 — Builder doc #2872** [P, draftable now; validate after W5/W6]. Docs only.
- **Housekeeping (F1–F4)** [P, independent, tiny]: rescope nip47 ownership claim (F1); fix PaymentPort name+prose (F2, fold into W11); refresh `docs/nips.md:54` (F3); reword `design:474` (F4).

### Suggested execution order
- **Wave A (parallel):** W1 (mint-HTTP), F1/F3/F4 housekeeping, W12 draft.
- **Wave B (parallel after A):** W3 (NWC backend, mint-independent), W2 (Cashu backend, needs W1).
- **Wave C (ONE shared worktree / single PR — heavy shared-file coupling on `nmp-wallet` `lib.rs`+new `register.rs`+backend selection):** W4 → W5 → W6 → W7 in sequence. Do NOT fan these out to separate PRs; they pairwise-conflict.
- **Wave D (parallel after C):** W13, W8, W9, W10, W11 (W11 carries the F2 fix).
- **Wave E:** validate W12 against real symbols; add mock-mint + mock-relay integration tests proving no private socket path; restart/reconcile tests at runtime level.

Net: the money-critical journal spine and all type/shape contracts are landed and internally coherent. The only substantive coherence defect is F2 (PaymentPort ownership drift + name collision); F1 is a real-but-gate-invisible double-exclusive; F3/F4 are stale-doc cleanups.
</teammate-message>
