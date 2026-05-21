# Chirp Web Runtime

The browser worker first tries to load a generated `nmp-wasm` package from:

```text
public/nmp-wasm/nmp_wasm.js
```

That file is optional for normal web builds. When it is absent, the worker emits
`wasm_bridge_unavailable` and falls back to `DegradedRuntime` with
`browser_bridge_unavailable` status.

If the generated module loads, the worker routes requests through
`NmpWasmRuntime.handle_json()`. Any `browser_actor_driver_missing` status then
comes from the real wasm runtime, which means the JS/wasm bridge is available
but the browser actor driver is still not linked.
