# Removed v2 Trait-Family Proposal

The old `DomainModule`, `ViewModule`, `IdentityModule`, `ModuleRegistry`,
generated `AppAction` / `AppUpdate` / `ViewSpec` enum, `FfiApp`, and
`nmp gen modules` design was removed. The old central view-catalog subpages
were removed with it.

Current seams are `ActionModule`, `register_event_observer`,
`register_typed_snapshot_projection`, capabilities, `NmpAppBuilder`, and
`nmp-defaults` composition.
