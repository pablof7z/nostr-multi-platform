# Subscription Compilation: Audit Gates

Planner-correctness gates live in `nmp-testing` integration tests. They assert
on the shape and identity of compiler output, not on modeled performance.

## Required Invariants

- Source replacement closes withdrawn child interests before the next compile
  drain.
- Empty source output fails closed and never becomes wildcard acquisition.
- Two source sessions that materialize the same child interest deduplicate to
  one wire REQ per relay.
- Component/read-model dependent interests for profiles, event ids, and
  addresses use the same planner/router/cache path as feed acquisition.
- Account switch re-runs active-account source reduction and closes the prior
  account's materialized children.
- Publish paths expose explicit override operations only through typed action
  namespaces that diagnostics can identify; ordinary publish actions do not
  carry ad hoc relay lists.

## Rule

Do not depend on generated per-app enum reflection. The generator path was
removed; audit tests should inspect live registries, action namespaces, planner
output, and emitted wire frames.
