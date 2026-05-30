# 00 — How to read this guide

Two audiences. One promise.

**The promise:** if you follow the patterns here, the hardest classes of
Nostr bugs become structurally impossible — not documented as footguns, not
caught by a linter, but ruled out by the type system, the actor model, and
the FFI surface.

- **Builders** — you came from NDK, Applesauce, or raw `nostr-sdk` and want to
  ship a Nostr app without re-implementing outbox routing, kind:3 tracking,
  reconnect replay, and reactivity for the hundredth time.
- **Agents** — you are an LLM extending the kernel. The doctrine D0–D10 is the
  contract you cannot violate; every section ties its claims to enforced code
  or marks them aspirational.

Each section header carries an **audience** tag (`builders` / `agents` /
`both`) and a **status** flag. Read the status flag first — it tells you
whether the section describes code you can run today or a milestone not yet
landed.

## Status legend — read this before trusting any code block

| Flag | Means | What it cites |
|---|---|---|
| **SHIPS** | Code exists and runs on master today | `crates/`/`apps/`/`ios/` `path:line` + any `docs/**` |
| **LANDED** | Designed + scaffolded, not feature-complete | design docs, ADRs, partial `crates/` cites |
| **PLANNED** | Scoped, not built | plan files + ADRs only — never a `crates/` `path:line` |

A `[PENDING M_n]` marker inside a SHIPS section means *that one bullet* awaits
milestone _n_; the surrounding section still ships. When a section says a
thing is aspirational, **that is not a doc bug** — it is the status flag doing
its job. (See the anti-patterns below.)

Authoritative status lives in `docs/plan.md`, `docs/BACKLOG.md`, and `WIP.md`.
This guide's numbered section files are the guide source; there is no parallel
builder-guide plan file. Doctrine canon: `docs/product-spec/doctrine.md`
(D0–D10, conflicts resolve in listed order).

## Two reading paths

Pick the one that matches your goal. Sections are numbered by dependency, not
by importance — out-of-order reading will leave you missing prerequisites.

### Path A — "I want to ship an app" (builders)

```
01 what NMP is ──▶ 02 mental model (kernel + extension seams) ──▶ 05 traits + seams
                          │
                          ▼
   25 migrate from NDK/Applesauce   17 iOS shell ──▶ 19 walkthrough
                          │                                 │
                          ▼                                 ▼
        21 framework-magic contract            26 FAQ / troubleshooting
```

Start at **01 → 02 → 05**, then jump to **19** (build a microblog
end-to-end). **25** is the fast on-ramp if you already think in NDK or
Applesauce terms. **21** tells you exactly what you get for free (so you do
not re-implement it). **26** when something breaks.

### Path B — "I want to extend the kernel" (agents)

```
03 doctrine D0–D10 ──▶ 02 mental model (kernel + extension seams) ──▶ 04 actor / TEA
        │                                      │
        ▼                                      ▼
05 traits + seams ──▶ 06 reactivity ──▶ 07 planner ──▶ 08 EventStore
        │                                                    │
        ▼                                                    ▼
20 add a protocol module      18 testing      22 doctrine checklist
        │                                              │
        ▼                                              ▼
27 doc/code discrepancies  ◀───────────────────────────┘
```

Start at **03** (the doctrine is the law) → **02** (where things live) →
**04/05/06/07/08** (the substrate). Use **20** (`nmp-nip29` as the canonical
reference) when adding a protocol module, **22** as the PR-review gate, and
file every doc/code gap you find into **27**.

## Section dependency graph (read upstream before downstream)

```
00 ─▶ 01 ─▶ 02 ─┬─▶ 03 ─▶ 22
                ├─▶ 04 ─▶ 17
                ├─▶ 05 ─┬─▶ 06 ─▶ 18
                │       ├─▶ 07 ─▶ 08 ─▶ 09
                │       ├─▶ 11 ─▶ 12
                │       ├─▶ 16
                │       └─▶ 20
                ├─▶ 10 (07,11)   13 (07,08)   14 (07,12,13)
                ├─▶ 15 ─▶ 19
                └─▶ 21 ─▶ 25 ─▶ 26
23 glossary · 24 reference cards — random-access; bookmark, do not read linearly.
27 discrepancies — the orchestrator queue; consult, do not "fix in section."
```

`(a,b)` after a node = also depends on sections a and b.

## Filing a doc bug — the section 27 route

Do **not** patch a section to "correct" what its status flag already marks as
aspirational. If you find a place where docs claim more than the code on
master delivers, file it in **[27 — Doc/code discrepancies](27-discrepancies.md)**
with five fields: *claim · evidence (`path:line`) · status · owning
milestone · severity*. The orchestrator drains that queue. Most entries are
"milestone not landed yet" or a deliberate post-v1 scope deferral (M9 DMs,
M12 Wallet) — not bugs.

## Anti-patterns

- **Reading sections out of order.** The graph above is a dependency order,
  not a suggestion. Section 12 (publishing) assumes 05 (traits + seams) and
  11 (signers); skipping them yields confusion that looks like a doc error.
- **Copying PLANNED code into a real app.** PLANNED sections cite plan files,
  never `crates/` `path:line`. There is no code behind them yet. Treat their
  code blocks as design intent, not API.
- **Assuming a section is "wrong" when the status flag marks it aspirational.**
  A `[PENDING M_n]` bullet or a LANDED flag is the doc telling the truth
  about reality. File it in 27 only if reality and the flag *disagree*.
- **Trusting `README.md` over `docs/plan.md` for milestone detail.**
  README is a snapshot; `docs/plan.md`, `docs/BACKLOG.md`, and `WIP.md` are the
  canonical planning/status surface. They should agree — if they do not, file a
  27 entry.
- **Skipping the doctrine (03) because you "only want to ship an app."**
  Builders who ignore D1/D4 re-introduce the spinner-gating and parallel-cache
  bugs NMP exists to prevent.

See also: [01 — What NMP is + why it exists](01-what-nmp-is.md), [02 — Mental model — kernel + extension seams](02-mental-model.md), [22 — Doctrine compliance checklist](22-doctrine-checklist.md).
