# LMDB Sub-Design: GC Working-Set Policy

GC is bounded and explicit. The store reaps correctness deletes and tombstones,
and durable LRU deletion is enabled only by an explicit finite retention budget.

## Current Model

`gc_step_with_pins(budget, now_secs, pins)` is the authoritative path. The
kernel derives `pins` from live state immediately before the GC pass. The store
does not persist claim ownership.

`gc_step(budget, now_secs)` is a convenience wrapper that passes an empty pin
set.

## Budget

`GcBudget` constrains how much work one pass may do. Production defaults reap
expired events and old tombstones but do not delete valid fetched events just
because they are cold.

Finite durable-retention policies must use the same pin and coverage guards as
the default path.

## Diagnostics

Every pass returns `GcReport` with counts and duration. The kernel records the
last report so GC remains observable.
