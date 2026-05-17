# FFI & iOS Bridge Debt Inventory — Detailed Findings (M10.5)

See [`debt-inventory.md`](./debt-inventory.md) for the summary table,
must-fix list, and conclusion.

---

## Findings F1–F16

### Finding F1: Unsafe FFI Pointer Dereference — `ffi.rs:75`

**File:** `crates/nmp-core/src/ffi.rs:73–79`
**Severity:** Medium (correct code, documentation gap)
**Type:** Unsafe without safety comment

```rust
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        unsafe { drop(Box::from_raw(app)); }
    }
}
```

**Classification:** **ACCEPTABLE**. Standard FFI ownership transfer.

**Recommendation:**
```rust
// safe: caller guarantees app is a valid pointer allocated by nmp_app_new()
unsafe { drop(Box::from_raw(app)); }
```

---

### Finding F2: Unsafe Pointer Dereference — `ffi.rs:275`

**File:** `crates/nmp-core/src/ffi.rs:271–277`
**Severity:** Medium (correct code, documentation gap)

```rust
fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() { None } else { Some(unsafe { &*app }) }
}
```

**Classification:** **ACCEPTABLE**. Null check makes this safe.

**Recommendation:**
```rust
// safe: caller guarantees non-null app is a valid NmpApp pointer
Some(unsafe { &*app })
```

---

### Finding F3: Unsafe C String Conversion — `ffi.rs:284`

**File:** `crates/nmp-core/src/ffi.rs:279–290`
**Severity:** Low (standard C FFI pattern, validated downstream)

```rust
unsafe { CStr::from_ptr(ptr) }.to_str().ok()...
```

**Classification:** **ACCEPTABLE**. `to_str().ok()` validates UTF-8.

**Recommendation:**
```rust
// safe: caller guarantees ptr is a valid null-terminated C string;
// to_str() rejects invalid UTF-8.
unsafe { CStr::from_ptr(ptr) }
```

---

### Finding F4: Allow Dead Code Suppression — `relay_worker.rs:242`

**File:** `crates/nmp-core/src/relay_worker.rs:240–245`
**Severity:** Low (defensive programming, clarity gap)

```rust
#[allow(unreachable_patterns)]
_ => {}
```

**Classification:** **ACCEPTABLE**. Defensive against future tungstenite
enum variants.

**Recommendation:**
```rust
// Stream type may gain additional TLS variants in future tungstenite versions
#[allow(unreachable_patterns)]
_ => {}
```

---

### Finding F5: Saturation Casts — `kernel/status.rs:117`

**Severity:** Low — intentional, bounded, explicit overflow handling.

```rust
refcount: claim_count.min(u32::MAX as usize) as u32,
```

**Classification:** **ACCEPTABLE**. Saturation is explicit; no silent overflow.

**Recommendation:** No change required.

---

### Finding F6–F10: Count-to-Metric Casts

**Files:** `kernel/update.rs:43–56` (5 casts), `kernel/nostr.rs:85` (1 cast).
**Severity:** Low — bounded by working-set design constraints.

**Classification:** **ACCEPTABLE**. Max 5,000 events (ADR-0001), max 500
visible (FFI clamp). No overflow risk.

**Recommendation:** No change required.

---

### Finding F11–F12: Relay Counter Saturation Casts

**Files:** `kernel/ingest.rs:13,20`, `kernel/requests.rs:572`.
**Severity:** Low — safe saturation arithmetic.

```rust
relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
```

**Classification:** **ACCEPTABLE**. Saturation is intentional for overflow safety.

**Recommendation:** No change required.

---

### Finding F13–F14: C FFI Bounds Casts — `ffi.rs:94,296`

**Severity:** Low — caller contract guaranteed, clamped bounds.

```rust
context: context as usize,          // void* → usize
visible_limit.clamp(1, 500) as usize // c_uint → usize
```

**Classification:** **ACCEPTABLE**. Standard FFI. Clamp bounds the second.

**Recommendation:** No change required.

---

### Finding F15–F16: Expect Calls — `kernel/status.rs:225,231`

**Severity:** Low — invariant maintained by construction.

```rust
self.relays.get(&role).expect("relay health initialized for every role")
```

**Classification:** **ACCEPTABLE**. `Kernel::new()` initializes all roles.

**Recommendation:** No change required.

---

## Doctrine Audits D0–D5

### D0: Kernel Never Grows App Nouns — COMPLIANT

- `nmp-core/src/kernel/` contains only Nostr-agnostic data structures.
- Nostr-specific parsing isolated in `kernel/nostr.rs` as utility functions.
- Substrate trait families define no concrete Nostr types.
- iOS bridge decodes payloads without leaking type names into kernel API.

---

### D1: Best-Effort Rendering with Placeholders — COMPLIANT

- `ProfileViews.swift:51` renders `ProfileCard.placeholder(pubkey:)` immediately.
- TimelineRow / avatar components render initials + color fallback without spinners.
- `KernelModel` projection cache keeps previous views visible while data is in-flight.

---

### D2: Reactivity Contract (Composite Reverse Index) — COMPLIANT

- `kernel/mod.rs` maintains `wire_subs` and `profile_claims` reverse indexes.
- All message handling routes through kernel methods updating index state.
- No projection bypass; all views computed via `kernel/update.rs::make_update()`.
- iOS `KernelModel.apply(result:)` runs on `@MainActor`.

---

### D3: Errors Never Cross FFI — COMPLIANT

- All FFI exports are void-returning C functions.
- No `Result` types or error enums in the FFI surface.
- Error states communicated via `RelayStatus.last_error` / `last_notice` JSON fields.
- iOS reads advisory diagnostics; no control flow decisions at the boundary.

---

### D4: One Writer Per Fact — COMPLIANT

- Kernel actor is single-threaded (one OS thread from `ffi.rs:51`).
- All state mutations route through `run_actor()` via a single `mpsc::channel()`.
- Relay workers only send events back to actor; never mutate kernel state directly.
- iOS bridge is `@MainActor`-annotated; SwiftUI updates serialized.

---

### D5: Capabilities Report, Never Decide — COMPLIANT

- iOS FFI bridge exposes capability requests as simple commands with no decision logic.
- Kernel processes them as `ActorCommand` variants routing to domain logic.
- No policy decisions in `KernelBridge.swift` or the FFI wrapper.
- `CapabilityModule` trait defines how modules *report*, not how bridge decides.
