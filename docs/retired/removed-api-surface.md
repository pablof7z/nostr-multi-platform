# Removed API Surface

The v2 `DomainModule` / `ViewModule` / `IdentityModule` / `ModuleRegistry`
proposal was removed. Current extension seams are `ActionModule`,
`register_event_observer`, `register_typed_snapshot_projection`, capability
sockets, and app-owned Rust composition roots.

The `nmp gen modules` per-app FFI generator, `apps/fixture`, and
`fixture-todo-core` walkthrough were removed. Current starters use `nmp init`
plus app-owned Rust composition that installs `nmp-substrate` and selected
protocol/app features explicitly.

The public JSON `nmp_app_dispatch_action` doorway is retired for production
writes. Current production write transport uses ADR-0064 dispatch bytes through
`nmp_app_dispatch_action_bytes` or the equivalent wasm/browser channel.
