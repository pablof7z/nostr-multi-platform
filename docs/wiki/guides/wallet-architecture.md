---
title: Wallet Architecture and Money-Safety
slug: wallet-architecture
topic: wallet-architecture
summary: "The wallet architecture uses a single WalletBackend trait with two backends: NWC (NIP-47) for Lightning/BOLT-11 and Cashu (NIP-60) for ecash"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:5ad70acc-1442-4343-92a7-f79b2fc59071
  - session:04745411-a0c1-4523-ac83-71dc983f410b
  - session:b46b47eb-a058-4f19-9451-13531c02c3bb
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
---

# Wallet Architecture and Money-Safety

## Wallet Backend Architecture

The wallet architecture uses a single WalletBackend trait with two backends: NWC (NIP-47) for Lightning/BOLT-11 and Cashu (NIP-60) for ecash. A composition layer selects which backend handles a given action. Zaps (NIP-57) emit a 'pay this' intent through a PaymentPort — the protocol-neutral seam in nmp_core::substrate through which nmp-nip57 emits PaymentIntent without depending on nmp-nip47 — and the selected backend fulfills it. The PaymentPort adapter wiring stays in nmp-nip47, not moved to nmp-wallet. Neither the caller nor the PaymentPort knows or cares which backend handles the intent. The nmp-nip57 crate already emits PaymentIntent through nmp_core::substrate::PaymentPort with no nmp-nip47 dependency, confirming the PaymentPort seam is real and protocol-neutral. Backend selection for NIP-57 zaps is done by supplying the selected backend's Arc<dyn PaymentPort> via nmp_nip57::Config::with_payment_port, passing nmp-nip47's port through — nmp-wallet does not need to define a second PaymentPort impl.

The WalletBackend trait lives in nmp-wallet, a Layer-4 composition crate that owns the WalletBackend seam, the durable operation journal, the event-sourced reducer, and the causal trail, composing NIP-47 and NIP-60 backends without introducing wallet-specific nouns into nmp-core. The nmp-wallet crate and the full WalletBackend seam — the trait, capability flags, payment_port.rs with pay_bolt11, projection, and the journal spine of saga/fact/trail/ledger — exist on origin/master via commit b746d3f (PRs #2876/#2877). The nmp-wallet crate also owns authoritative relay selection (kind:10019 with NIP-65 fallback). A dead WalletBackend trait whose pay_invoice always returned Err(Unsupported) was previously defined in nmp-nip60 and has been deleted from that crate — it belongs in nmp-wallet, not in nmp-nip60. The nmp-wallet crate is parked/excluded from the default workspace (same posture nmp-nip60 had) since its actions aren't shippable yet.

The WalletBackend trait is command-shaped with methods start_intent, on_wallet_event, and on_mint_result, has no blocking FFI calls, and includes a reference stub implementation. The WalletBackend seam is actor-owned, command-shaped, returns Vec<ActorCommand>, never blocks across FFI, and uses a capability-flag model where an absent capability is not a runtime failure. The on_wallet_event/on_mint_result methods on WalletBackend are documented no-ops for the NWC adapter, not silently skipped. The generic on_wallet_event(&KernelEvent) -> Vec<ActorCommand> seam cannot drive real kind:23195 reconciliation because nmp-nip47's actual decode path needs raw relay frame text plus the connection's NIP-04/44 secret and returns Vec<OutboundMessage> — reconciliation stays through nmp-nip47's existing interceptor until actor-wiring bridges it.

The kind:17375 relay tags are non-authoritative relay hints, not authoritative relay selection. Authoritative relay selection is kind:10019 plus NIP-65 fallback, owned by nmp-wallet. legacy_relay_hint is decode-only and must not be emitted as a relay tag by build_wallet_event. publish_nutzap_info takes an explicit relays parameter rather than using legacy relay hints. In nmp-nip60, the WalletConfig::relays field and the Nip60WalletHandle::relays() accessor have been renamed to legacy_relay_hint to reflect their demoted, non-authoritative status. The NIP-61 NutZapInfo.relays field, by contrast, remains authoritative and untouched — it is separate from the demoted kind:17375 relay hints.

The wallet design doc ownership model must include nmp-nwc alongside nmp-wallet, cite the nmp-note-feed precedent for the new Layer-4 crate, justify or drop the nmp-nip57 dependency, and fix composition-owner vs composition-root terminology. The wallet design doc must not carry compat-alias migration language: the nwc.* names are a Phase-2 hard-break with no alias period. Wallet action names must have no compatibility aliases — nmp.wallet.connect/disconnect/pay_invoice are the single canonical names, not alias pairs. Renaming wallet actions to the aspirational nmp.wallet.nwc.connect/nwc.disconnect names requires moving nmp-nip47's ActionModule + wire-schema registration and is deferred to Phase 2 (NWC consolidation).

The wallet capability flags are exactly the 8-flag list: pay_bolt11, create_cashu_wallet, publish_nutzap_info, send_nutzap, redeem_nutzap, deposit_cashu, melt_cashu, observe_nutzap_receipts. The NWC WalletBackend adapter exposes only pay_bolt11 as true in its capability set; all other capabilities are false. The NWC WalletBackend's start_intent for PayBolt11 emits the same WalletPayInvoiceCommand that nmp.wallet.pay_invoice's ActionModule already emits, and every Cashu/nutzap/select-backend variant is a documented Vec::new() no-op rather than a panic. The NWC backend's snapshot() derives WalletReadiness from nmp-nip47's shared WalletStatus slot, with the V-79 heartbeat connection_state checked before the raw status token and winning on Reconnecting/TransportLost. The NWC backend's balances stays empty because WalletBalanceRow is a per-mint Cashu row and a single-purse Lightning balance has no mint analog. Constructing a live NwcWalletBackend needs both a WalletRuntimeHandle and a WalletStatusSlot clone bound to the same runtime instance, but nmp_nip47::register()'s returned Handles only exposes the runtime handle, not the status slot — wiring this end-to-end needs a small nmp-nip47 change to add Handles::status.

Epic #2864 is the NIP-60/61 wallet epic, with Phase 0 done (nmp-nip60 reactivated, PR #2866 merged) and Phase 1 scoped to building the nmp-wallet composition crate. The wallet milestone (#1001) is deferred until post-1.0 scheduling.

nmp-wallet is a Rust crate containing real, tested wallet code including action-name constants, WalletCapabilities, WalletProjection, the operation-journal state machine, and the WalletBackend trait. Only nmp.wallet.connect/disconnect/pay_invoice (NWC) are actually dispatchable today via nmp-nip47; the Cashu/nutzap action names and the new WalletProjection/WalletCapabilities/journal types are real, tested, frozen-contract code but not yet wired to a live backend.

NIP-87 (Ecash Mint Discoverability) uses kind:38172 mint announcements and kind:38000 recommendations for mint discovery.

<!-- citations: [^1c293-9787e] [^04745-5fac8] [^91a86-53cba] [^91a86-8ba7f] [^91a86-2e0e7] [^5ad70-0ae05] [^5ad70-6cb95] [^91a86-c7c44] [^91a86-13a34] [^91a86-0c9ae] [^91a86-ee169] [^b46b4-0ca3e] [^91a86-48c95] -->
## Rust Owns All Wallet Policy and Secrets

Rust owns all wallet policy and secret material. Native and browser shells never choose mints, relays, or retry policy; they render a bounded wallet projection (balance, capabilities, pending ops) and fire typed actions (create wallet, send nutzap, redeem, pay invoice). All secret material — Cashu proofs and the wallet private key — stays inside Rust and never crosses the FFI boundary.

<!-- citations: [^91a86-f5465] [^91a86-fb9c1] -->
## Nostr Events as Wallet State

Nostr events are the wallet state. The wallet config (kind:17375) and token balances (kind:7375) live as NIP-44-encrypted Nostr events on relays, syncing wallet state across devices via Nostr with no local-only wallet. Rust owns the policy and money-safety; the mint is just an HTTP dependency behind a backend trait; and everything routes through the normal kernel/relay path with no side-door wallet stack.

<!-- citations: [^91a86-0850b] -->
## Receive Policy

The wallet receive policy is public via kind:10019 Nostr events, specifying trusted mints, target relays, and the Cashu P2PK key. Senders read these events to know how to pay the recipient.

<!-- citations: [^91a86-af934] -->
## Sending a Nutzap

Sending a nutzap means minting P2PK-locked ecash proofs to the recipient's key and publishing them as kind:9321 events to the recipient's listed relays. Nutzap send resolves the recipient's kind:10019 via kernel reads, only sends to recipient-listed mints with the exact u tag, publishes to listed relays, and fails closed on no trusted mint, no P2PK, or no reachable relay.

<!-- citations: [^91a86-c6dfe] [^91a86-4660c] -->
## Receiving a Nutzap

Receiving a nutzap requires validating the right mint, the DLEQ proof, and the P2PK lock to the recipient's key. The proofs are then swapped into fresh wallet-owned ones and marked redeemed. Received nutzap proofs are never republished. Nutzap receiving subscribes to kind:9321 events filtered by #p and #u tags limited to accepted mints, and verifies p-tag, mint, P2PK, and DLEQ before counting — unverifiable proofs are rejected and never counted.

<!-- citations: [^91a86-22774] [^91a86-9ff78] -->
## Fail-Close Behavior

The wallet fail-closes everywhere. An unknown mint, missing DLEQ, mismatched P2PK lock, or a signer that cannot do NIP-44 results in the capability being simply absent rather than a button that errors. <!-- [^91a86-3a370] -->

## Money-Safety Journal

The wallet operation journal is a durable write-side saga with states Draft → MintPending → MintSettled → PublishPending → Settled (plus Unknown/Failed). Its states only advance (monotonic, effectful semantics) and its whole job is crash-recovery and reconciling against external mint authority so no double-spend or double-pay survives process death. The journal guarantees at-most-once correctness, which is a category mismatch with Trellis's convergent, declarative read-side reconciliation. Restart reconciliation connects the journal's Unknown→PublishPending→Settled reconcile path to real backend/mint probes at startup.

The journal must not be built on Trellis, because Trellis graphs are in-memory, per-session, and die with the process by design, while the journal's entire purpose is surviving process death after an irreversible external mint spend. Trellis's deterministic replay is a derivation-consistency oracle over local inputs, whereas wallet recovery reconciles against a remote mint authority whose answers are not a function of local state.

The journal is an actor-owned durable journal in nmp-wallet, persisted through NMP storage, following the standard NMP actor pattern: command-shaped reducers that never await mint HTTP, capability-lane mint workers returning raw results, and the existing publish engine handling PublishPending.

The money-safety saga (concern 1) writes pre-effect records that serve as the at-most-once mechanism, is durable, and is money-critical. The causal trail (concern 3) writes post-observation records that are diagnostic and bounded. The wallet derived state (concern 2) is the fold over post-observation facts. The saga and trail are producer/consumer, not subset: the saga emits facts into the trail but their schemas never merge, because money-critical pre-effect records must not live in a diagnostic log with bounded eviction.

A generic durable-saga substrate should not be built now as a one-consumer abstraction. If a second saga ever appears (nmp-marmot's pending-MLS-autopublish is the likely candidate), the substrate should be extracted post-hoc at that point.

<!-- citations: [^91a86-f06cb] [^91a86-3b3f5] [^91a86-f4fd1] [^91a86-3f79b] [^91a86-17771] [^91a86-921f2] -->
## Read Projection

The wallet read projection — balances and pending-op summaries keyed by correlation id — is a bounded typed projection that rides the normal session path. It may invisibly sit on Trellis-backed reconciliation without the wallet seeing it. The wallet projection is bounded to MAX_WALLET_PROJECTION_ROWS=100 with keep-last truncation. Trellis legitimately stays in the wallet picture only for the read projection riding the normal session path invisibly — never for the money-safety journal or the causal trail.

Capability-gated UI hides controls when the capability is absent rather than producing a runtime failure, with the action_namespaces() enforcement test cited as evidence.

<!-- citations: [^91a86-458be] [^1c293-f6627] [^91a86-8e238] -->
## Three Wallet-State Concerns

Wallet state separates into three distinct problems: (1) the money-safety saga — a durable persisted journal for at-most-once crash recovery; (2) the current state — a derived convergence problem over racing deletions, probes, and arrivals riding the read path; and (3) the causal trail — an in-memory annotated delta log answering why the state is its current shape. The money-safety journal persists separately with different correctness rules; the current state and causal trail are read-path constructions and are not the journal. <!-- [^91a86-67679] -->

## Event-Sourced Wallet Reducer

The wallet reducer is event-sourced and confluent: current state is a fold over an ordered stream of typed facts (TokenAdded, TokenDeleted, MintProbed, NutzapRedeemed, SagaTransition, StateRebuilt), with each fold step retaining a bounded in-memory ring of what changed and why. Terminal state is order-insensitive — a delete arriving before its token must tombstone, not no-op — so two devices show the same balance, even though the causal trail itself is order-sensitive. Both the fold and rebuild_from paths route through shared apply_token_live/apply_token_tombstone guards so confluence holds in both. The MintProbed Spent verdict is absorbing: a stale Unspent probe cannot resurrect balance.

The three wallet streams are distinguished by write moment: the saga writes pre-effect ('about to consume proofs for a mint spend') as the at-most-once mechanism; the trail writes post-observation ('a deletion arrived / a probe found C spent'); and the state is the fold over post-observation facts. The trail cannot be read out of the saga because the saga only knows locally-initiated operations, while most chaotic wallet events (inbound token arrivals, NIP-09 deletions from other devices, incoming nutzaps) never touch the saga. The relationship is producer/consumer — the saga emits facts into the trail but their schemas never merge, because money-critical pre-effect records must not live in a diagnostic log with bounded eviction.

The wallet causal trail (concern 3) is an in-memory, annotated delta log — rebuildable, non-money-critical — that answers 'what sequence of events produced the current wallet state.' An event log (ordered causal timeline) is the more direct and simpler primitive than Trellis's dependency-graph/trace machinery for answering that question. Trellis is doubly moot for the wallet causal trail because a timeline is not a dependency graph and ADR-0075 confines Trellis trace to dev-only tooling while 'why is my wallet this shape' is a product-surface question. The wallet-specific causal semantics — why a deletion fired, what a mint probe concluded — are Nostr/product meaning that lives in nmp-wallet, not in Trellis core, per ADR-0075's boundary. Trellis applies to the wallet read path for acquisition only (which relays/filters per kind:10019), never to proof-set derivation, which is NMP/actor-owned product meaning per ADR-0075 Ownership.

The durable causal tier is NIP-60 kind:7376 history events (already required by the design doc for every balance-changing op, with a codec in nmp-nip60/src/history_event.rs) plus the del field on token events — not a locally-built durable trail. Wallet restart rebuilds state by folding kind:7376 into coarse pre-session facts; then the in-memory ring accumulates fine facts from there, yielding two resolutions in one timeline. The in-memory delta ring is the session-local high-resolution overlay for mint-probe verdicts, arrival provenance, and saga correlation ids — details too fine-grained or transient for kind:7376. The in-memory delta ring is never a rebuild authority; restart rebuilds from Nostr events plus saga reconcile, entering as a StateRebuilt genesis fact.

The reducer folds the WalletFact stream two ways: a time-ordered bounded delta ring answering what sequence produced the current shape, and a per-atom last-cause index (token_event_id → last WalletFact that touched it) answering why a specific proof/token is present. The per-atom index is O(current state), not O(traffic), so a nutzap flood cannot evict the cause of a token still held.

WalletFact provenance is typed as Relay(RelayRef), Saga(CorrelationId), or MintRollover. WalletFact DeleteCause is typed as Nip09Delete { by: EventId } or LocalRollover { op: CorrelationId }. Wallet fact payloads carry only event ids, op ids, amounts-by-unit, and canonical mint URLs, and only when already public via kind:10019; never proofs or secrets. This privacy constraint is enforced by exhaustive-match plus a sealed marker trait over field types, not a runtime-only check.

The wallet journal spine file split is saga.rs / fact.rs / trail.rs / ledger.rs plus ledger_state.rs (split for the 500-LOC hard cap). TokenAdded carries proofs: Vec<ProofAtom> not a flat amount, so the reducer owns proof-set membership, not just token-event membership. The saga emits an explicit WalletSagaEvent from transition() (not a callback/drain) with one-directional producer wiring into WalletFact::from. rebuild_from is a distinct entry point from apply(), folding seeds into one StateRebuilt genesis fact. WalletDerivedState is deliberately non-Serialize; the durable tier is kind:7375/7376, not the derived state.

The nmp-wallet crate owns this event-sourced, confluent reducer; state is the fold, the causal trail is a bounded in-memory ring plus a per-atom cause index over the same facts, the saga feeds facts in as a producer, and kind:7376 is the durable tier — all separate from the money-safety journal's persisted at-most-once records.

<!-- citations: [^91a86-aa2e8] [^91a86-633f9] [^91a86-8be00] [^91a86-7c7bd] [^91a86-f76a2] [^91a86-4c25c] [^91a86-ced00] [^91a86-afbec] -->
## Phase-1 Scope and Testing

The Phase-1 agent prompt is scoped to the money-critical journal/reducer/trail spine only; later Phase-1 items (backend selection policy, PaymentPort adapter, full action namespace, wallet projection) are follow-on PRs under epic #2864.

The wallet Phase-1 implementation launches in waves. Wave 1 handles disjoint file sets: the design doc landing, nmp-nip60 cleanups, and the journal/reducer/trail spine. Wave 2 handles downstream items (backend selection, PaymentPort adapter, action namespace, projection wiring) that depend on the spine's types. The three Wave-1 wallet agents have disjoint file ownership: wallet-design owns docs/architecture/nip60-nip61-wallet-design.md and docs/nips.md, nip60-cleanups owns crates/nmp-nip60/, and wallet-spine owns crates/nmp-wallet/.

Wallet Phase-1 tests are scoped to `cargo test -p nmp-wallet -p nmp-nip60` plus `cargo test -p nmp-testing --test doctrine_lint_smoke`, and `cargo test --workspace` must not be run. Wallet implementation work uses an isolated worktree, a branch and PR, never pushing to master, with the worktree cleaned up when done.

Wallet Phase-1 acceptance tests must prove confluence and restart-reconcile with tests for out-of-order delete, mint-probe-found-spent, and crash-after-MintSettled. The MintProbed Spent verdict is absorbing: a stale Unspent probe cannot resurrect balance. Both the fold and rebuild_from paths route through shared apply_token_live/apply_token_tombstone guards so confluence holds in both.

The wallet action namespace has no duplicated action constants: no two wallet action constants share a string value, enforced by a test.

The refined wallet journal/reducer/trail design is captured as PR #2869 (a design note appended to the canonical architecture doc `docs/architecture/nip60-nip61-wallet-design.md`) with a pointer comment on epic #2864.

PR #2871 (branch codex/2864-wallet-phase1-slice) scaffolds the nmp-wallet crate with backend.rs, capability.rs, journal.rs, payment_port.rs, projection.rs, and ownership and release-manifest wiring. However, PR #2871's journal.rs contains only the older linear saga (WalletOperationState with Draft, Prepared, MintPending, MintSettled, PublishPending, Settled, Unknown, Failed plus WalletOperationJournal) and does not implement the #2869 three-concern design — no WalletFact reducer, no causal trail ring, no per-atom cause index, no StateRebuilt, no confluence. Additionally, PR #2871 carries a compat-alias violation: its tradeoffs state nmp.wallet.connect/disconnect/pay_invoice remain compatibility aliases, which violates the zero-tolerance no-compat-aliases rule and requires a hard-break to the canonical backend-specific namespace with all callers updated in the same PR.

The nmp-wallet crate and the full WalletBackend seam now exist on origin/master via commit b746d3f (PRs #2876/#2877), so downstream waves build against the existing trait rather than creating the crate. W3 (#2886, the NWC backend) is unblocked and must build impl WalletBackend + backend/nwc.rs against the existing trait on master; it does not wait on Wave C. W1 (#2885, mint-HTTP) is correctly scoped to existing crates and should proceed as planned. Wave C only wires actions/projection/actor into nmp-wallet; it does not create the crate. #2882 (release-classify) is the real blocker to the nutsack green end-to-end and should be pulled forward.

A final coherence pass across the whole NIP-60/61/wallet surface (design doc ↔ epic ↔ crates ↔ nips.md) is performed so it all reconciles.

The builder guide doc docs/builder-guide/29-nip60-wallet.md was authored on the codex/2872-wallet-builder-doc branch and opened as PR #2888. It covers the two things a builder does: dispatch nmp.wallet.* actions with a correlation id and bind the bounded "wallet" projection. It includes send-nutzap and receive/redeem-nutzap walkthroughs explaining kind:10019 resolution, fail-closed states, the operation journal, and the single publish chokepoint. The builder guide contains no Trellis vocabulary and no secrets in any example.

The builder guide doc's status table distinguishes nmp.wallet.connect/disconnect/pay_invoice (NWC, dispatchable today via nmp-nip47) from Cashu/nutzap action names and WalletProjection/WalletCapabilities/journal types that are real, tested, frozen-contract code but not yet wired to a live backend. It specifies capability-gated UI where absent capability results in a hidden control, never a runtime failure. It explains relay-acquisition rules, the legacy_relay_hint non-authoritative field, and fail-closed rules. It adds a one-line citation link to docs/nips.md from the NIP-60/61 support-matrix row.

<!-- citations: [^1c293-72bc4] [^91a86-58b32] [^91a86-80ad6] [^91a86-2a34c] [^91a86-1690a] [^91a86-59d05] [^b46b4-cd084] [^1c293-7653d] -->
