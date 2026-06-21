---
type: episode-card
date: 2026-05-21
session: 27e05f9e-7508-4314-82dd-3f83f15b5d8f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/27e05f9e-7508-4314-82dd-3f83f15b5d8f.jsonl
salience: reversal
status: active
subjects:
  - nmp-website
  - site-voice
  - homepage-ia
supersedes: []
related_claims: []
source_lines:
  - 1109-1109
  - 1287-1288
  - 1299-1331
  - 1464-1476
  - 1478-1524
  - 1628-1653
captured_at: 2026-06-18T04:55:29Z
---

# Episode: NMP website direction reversal: code-first → opinion-first

## Prior State

The nmp.f7z.io website was a code-first, mechanism-heavy marketing site: 3-tab Swift/Kotlin/Rust code widget as hero, an install block with git clone command, a 4-pillar bug-class gallery, an architecture diagram, Chirp screenshot section, and a /method page with code-comparison before/after blocks. The homepage led with what the framework does (code, install, architecture).

## Trigger

User explicitly rejected the approach at line 1109: 'this is the wrong approach — I want the site to not be about code, I want the site to be about the philosophical underpinnings — its hard to read code and see if you agree or disagree with it by skimming it — but outright being explicit about the philosophical underpinnings is more interesting.' Further refined at line 1287: 'don't use "philosophical underpinnings" — show, don't tell… more of a basecamp, ben settle approach of non-neediness.'

## Decision

The entire site was reconceived as opinion-first, not code-first. The homepage now opens with two declarative lede statements ('A broken Nostr app should be impossible to build.' / 'Correctness failures in Nostr clients are framework defects. Not developer mistakes.') followed by nine standalone opinion statements with no expansion — just the void. No code on the homepage. No install command. No architecture diagram at the top. The /method page was rewritten from bug-gallery + code-comparison into a manifesto with ten rules, thirteen things the framework handles, and the architecture diagram demoted to a bottom section ('The runtime, drawn.'). Voice is declarative and non-needy — no 'our philosophy' heading, no preamble.

## Consequences

- Homepage components (Hero, InstallBlock, Pillars, Actor, ChirpSection, MethodTeaser) replaced by a Statement component rendering standalone opinions
- Header trimmed: 'Docs' link dropped, 'GitHub' renamed to 'Source'
- Footer trimmed: 'Protocol-first' tagline dropped, kept only 'Built on rust-nostr.'
- Code tabs, install commands, and architecture diagram removed from homepage entirely
- /method page now carries rules + principles + a single 'thirteen things' list instead of code before/after comparisons
- A durable research brief (_research/philosophy.md, 596 lines) was mined from ADRs, aim.md, and the user's tenex conversation corpus — this is now the canonical source for site copy
- The site voice doctrine is established: hold the opinion directly, never label it ('no Philosophy section heading')

## Open Tail

- The Statement component currently renders all nine opinions with uniform styling — some may deserve different visual weight or spatial treatment in future iterations
- The /method page still includes the architecture diagram at the bottom; may need evaluation whether even that is too mechanism-forward
- The Chirp section and its screenshot were removed from the homepage — whether to restore it as a proof-point elsewhere is unresolved

## Evidence

- transcript lines 1109-1109
- transcript lines 1287-1288
- transcript lines 1299-1331
- transcript lines 1464-1476
- transcript lines 1478-1524
- transcript lines 1628-1653

