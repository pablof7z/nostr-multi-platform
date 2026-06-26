# FFI Update Transport

The current runtime update transport is the FlatBuffers byte stream described in
[`docs/ffi-surface.md`](../ffi-surface.md). Hosts receive typed snapshot envelope
fields and typed projection sidecars from generated bindings.

JSON update-envelope experiments are not production transport guidance. JSON
remains appropriate for Nostr relay protocol frames, diagnostics, fixtures, and
explicit test tooling.
