# Intent-classed routing + NIP-50 search §4 — Planner integration

> Parent: `docs/design/intent-routing.md`.
> Cross-refs: type surface in `types.md` (§3); NIP-51 fact stream in
> `nip51-facts.md` (§5).

## 4. Planner integration

### 4.1 Per-author class-routing partition

The user's framing: *"my list is about where I publish and where I read
when not specifying authors; bob's list is when I want to see what wiki
bob published. If the REQ has authors:[bob, alice], it uses bob's 10102
for kinds:[30818], authors:[bob], alice's 10102 for kinds:[30818],
authors:[alice]."*

This drives the partition logic:

```
interest: shape = { kinds: [30818], authors: [bob, alice] }
       → class = owner-declared wiki class (from_kind(30818))
       → routing_family = PublisherKeyed
       → split per author:
           sub-shape A: { kinds: [30818], authors: [bob] }
                        relays = class_relays_for_author(<wiki class>, bob)
                                 .unwrap_or_else(|| nip65.write_relays(bob))
           sub-shape B: { kinds: [30818], authors: [alice] }
                        relays = class_relays_for_author(<wiki class>, alice)
                                 .unwrap_or_else(|| nip65.write_relays(alice))
```

When `class.routing_family() == Personal`, the partition does not split
by author — the active account's `class_relays_personal(class)` answers
the whole interest (used for future personal classes). When
`class.routing_family() == None`, the planner skips class routing entirely
and runs the existing four-lane partition.

**Search interests are NOT handled by `case_g_class_routed`.** A search
`InterestShape` (one with `search: Some(_)`) routes via the generic
four-lane planner with relay URLs supplied directly by `nmp-nip50` from
`SearchRelayListProjection`; the planner does not inspect `EventClass` for
search-bearing shapes. This is the higher-order model (ADR-0071 2026-06-22
amendment): search relay selection is performed entirely above the planner.

`case_g_class_routed` runs after `case_a_authors` and before
`case_e_relay_pinned`. NIP-29 events still take the `relay_pin` lane
because their `EventClass::GroupMessage` has `routing_family == RelayPin`,
and the partition cases gate on family.

### 4.2 New merge rule

**Rule 10 — `search` equality.** Two shapes refuse to merge unless their
`search` fields are equal (including both being `None`). Reasoning:
broadening a search would silently change semantics; narrowing would
lose results.

### 4.3 Blocked-relay post-filter (fail loud)

After all partition cases run and the per-relay plan is assembled, the
compiler subtracts `outbox.blocked_relays()` from every `RelayPlan`'s
relay URL set. If a relay is partially blocked, the plan emits a
diagnostic `RelayBlocked { url, removed_interests }` so the UI can
explain the shrinkage.

**If every relay in the plan is subtracted, the compiler returns
`PlannerError::AllRelaysBlocked`** — no silent empty plan. The publish
engine maps the equivalent error to `PublishOutcome::AllRelaysBlocked`.
This is a deliberate fail-loud choice (ADR-0071 decision 7); the UX
must surface this clearly because a user who blocked all their relays
by mistake will otherwise see nothing happen.

### 4.4 Lazy 10102 fetch lifecycle

When `case_g_class_routed` encounters a publisher-keyed class interest naming an author
whose kind:10102 hasn't been fetched yet:

1. The planner returns the current plan with that author's lane routed
   via NIP-65 fallback (so reads aren't blocked on the fetch).
2. The resolver enqueues a one-shot kind:10102 fetch for the author
   against the active account's read relays.
3. When the fetch completes (or EOSEs empty), the resolver invalidates
   the planner's cache for the affected interest, triggering a
   recompile that re-routes to the now-known wiki relays.
4. The kind:10102 subscription is kept alive (replaceable, tailing)
   for as long as any class-routed interest references the author.
   When the last interest ends, the subscription closes and the
   author's entry is evicted from the per-author fact cache.

This keeps the working set bounded by active view lifetimes.
