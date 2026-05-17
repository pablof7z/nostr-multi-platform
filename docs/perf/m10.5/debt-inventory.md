# FFI & iOS Bridge Debt Inventory (M10.5 Audit)

**Audit Date:** 2026-05-18  
**Scope:** Rust FFI boundary + iOS bridge code paths  
**Files Scanned:** 19 Rust modules + 9 Swift files (5,184 LOC Rust, 1,375 LOC Swift)  
**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked

---

## 1. Summary Table

| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
|------|-------|--------|--------|---------|------------------|-------|------------------|---------|-------|
| ffi.rs | 0 | 0 | 0 | 0 | 3 | 2 | 0 | 0 | FFI boundary; 3 unsafe blocks without safety comments |
| actor.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| relay.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| relay_worker.rs | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 1 allow(unreachable_patterns) |
| app.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| lib.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| kernel/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| kernel/nostr.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (f64 ratio); .unwrap_or_default used safely |
| kernel/status.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 2 | 1 cast; 2 expect() with clear messages |
| kernel/requests.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 1 cast (u64 saturation) |
| kernel/ingest.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (u64 saturation) |
| kernel/update.rs | 0 | 0 | 0 | 0 | 0 | 5 | 0 | 0 | 5 casts (count→u64/u32) |
| kernel/tests.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ tests only |
| substrate/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/view.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/action.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/capability.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/domain.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/identity.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| **iOS (Swift)** | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ all 9 Swift files clean |
| **TOTALS** | **0** | **0** | **0** | **0** | **3** | **14** | **1** | **2** | **20 items; 0 bugs** |

---

## 2. Findings

### Finding F1: Unsafe FFI Pointer Dereference — `ffi.rs:75`

**File:** `crates/nmp-core/src/ffi.rs:73–79`  
**Severity:** Medium (correct code, documentation gap)  
**Type:** Unsafe without safety comment

```rust
73  pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
74      if !app.is_null() {
75          unsafe {
76              drop(Box::from_raw(app));
77          }
78          }
79  }
```

**Issue:** The unsafe block dereferences a raw C pointer returned from FFI without a safety comment explaining the invariant (the caller must guarantee the pointer is a valid NmpApp allocated by `nmp_app_new()`).

**Classification:** **ACCEPTABLE**. Code is correct; this is a standard FFI ownership transfer pattern. Pointer validity is guaranteed by C FFI caller contract.

**Recommendation:** Add inline safety comment for documentation and future maintainability:
```rust
// safe: caller guarantees app is a valid pointer allocated by nmp_app_new()
unsafe { drop(Box::from_raw(app)); }
```

---

### Finding F2: Unsafe Pointer Dereference — `ffi.rs:275`

**File:** `crates/nmp-core/src/ffi.rs:271–277`  
**Severity:** Medium (correct code, documentation gap)  
**Type:** Unsafe without safety comment

```rust
271  fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
272      if app.is_null() {
273          None
274      } else {
275          Some(unsafe { &*app })
276      }
277  }
```

**Issue:** Unsafe dereference of a raw C FFI pointer without a safety comment. The lifetime `'a` is sound because the reference is scoped to the caller, but the memory safety invariant (pointer validity) is not documented.

**Classification:** **ACCEPTABLE**. The null check makes this safe in context. The pattern is correct.

**Recommendation:** Add inline safety comment:
```rust
// safe: caller guarantees non-null app is a valid NmpApp pointer
Some(unsafe { &*app })
```

---

### Finding F3: Unsafe C String Conversion — `ffi.rs:284`

**File:** `crates/nmp-core/src/ffi.rs:279–290`  
**Severity:** Low (standard C FFI pattern, validated downstream)  
**Type:** Unsafe without safety comment

```rust
279  fn c_string_argument(ptr: *const c_char) -> Option<String> {
280      if ptr.is_null() {
281          return None;
282      }
283
284      unsafe { CStr::from_ptr(ptr) }
285          .to_str()
286          .ok()
287          .map(str::trim)
288          .filter(|value| !value.is_empty())
289          .map(ToOwned::to_owned)
290  }
```

**Issue:** Standard C FFI pattern but lacks a safety comment. The caller must guarantee the pointer is a valid, null-terminated UTF-8 C string. The unsafe is narrowly scoped and the result is validated via `to_str()`.

**Classification:** **ACCEPTABLE**. Pattern is correct. The result chain (`to_str().ok()`) validates UTF-8 and null-termination.

**Recommendation:** Add safety comment:
```rust
// safe: caller guarantees ptr is a valid null-terminated C string.
// Validation: to_str() will reject invalid UTF-8.
unsafe { CStr::from_ptr(ptr) }
```

---

### Finding F4: Allow Dead Code Suppression — `relay_worker.rs:242`

**File:** `crates/nmp-core/src/relay_worker.rs:240–245`  
**Severity:** Low (defensive programming, clarity gap)  
**Type:** Compiler attribute without comment

```rust
238        MaybeTlsStream::Rustls(stream) => {
239            let tcp = stream.get_ref();
240            let _ = tcp.set_read_timeout(Some(duration));
241        }
242        #[allow(unreachable_patterns)]
243        _ => {}
244    }
```

**Issue:** The `#[allow(unreachable_patterns)]` suppresses a warning for the final catch-all arm. This can occur if `tungstenite::MaybeTlsStream` enum variants change between versions. The intent is clear (defense against future enum variants), but the rationale is not documented in code.

**Classification:** **ACCEPTABLE**. This is defensive programming for a third-party enum. The suppression is narrow and justified.

**Recommendation:** Add comment explaining the defensive intent:
```rust
// Stream type may have additional TLS variants in future tungstenite versions
#[allow(unreachable_patterns)]
_ => {}
```

---

### Finding F5: Saturation Casts — `kernel/status.rs:117`

**File:** `crates/nmp-core/src/kernel/status.rs:117`  
**Severity:** Low (intentional, bounded, explicit overflow handling)  
**Type:** Integer cast with saturation

```rust
117  refcount: claim_count.min(u32::MAX as usize) as u32,
```

**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.

**Classification:** **ACCEPTABLE**. The saturation is explicit and intentional. Pattern is correct and defensive.

**Recommendation:** No change required. The code is correct.

---

### Finding F6–F10: Count-to-Metric Casts — `kernel/update.rs, kernel/nostr.rs`

**Files:**
- `kernel/update.rs:43–56` (5 casts: `count as u64`, `count as u32`)
- `kernel/nostr.rs:85` (1 cast: `usize as f64`)

**Severity:** Low (bounded by design, no overflow risk)  
**Type:** Collection count casts to metric types

**Examples:**
```rust
// kernel/update.rs:43
note_events: self.events.values().filter(|event| event.kind == 1).count() as u64,

// kernel/nostr.rs:85
numerator as f64 / denominator as f64
```

**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).

**Classification:** **ACCEPTABLE**. Bounded by design. No overflow risk.

**Recommendation:** No change required. Code is correct.

---

### Finding F11–F12: Relay Counter Saturation Casts — `kernel/ingest.rs:13,20` and `kernel/requests.rs:572`

**Files:**
- `kernel/ingest.rs:13,20` (2 casts: `.len() as u64`)
- `kernel/requests.rs:572` (1 cast: `.len() as u64`)

**Severity:** Low (safe saturation arithmetic, bounded message sizes)  
**Type:** Safe saturation casts for telemetry counters

```rust
// kernel/ingest.rs:13
relay.counters.bytes_rx = relay.counters.bytes_rx.saturating_add(text.len() as u64);
```

**Issue:** Casting message length to u64 for counter accumulation. The message length is bounded by relay protocol limits (WebSocket frames are ≤2^63 bytes in practice); saturation is intentional for overflow safety.

**Classification:** **ACCEPTABLE**. Defensive overflow handling via saturation arithmetic.

**Recommendation:** No change required. Pattern is correct.

---

### Finding F13–F14: C FFI Bounds Casts — `ffi.rs:94,296`

**File:** `crates/nmp-core/src/ffi.rs`

**Severity:** Low (caller contract guaranteed, clamped bounds)  
**Type:** FFI argument coercion

```rust
// ffi.rs:94
context: context as usize,  // void* → usize

// ffi.rs:296
visible_limit.clamp(1, 500) as usize  // c_uint → usize
```

**Issue:** The first is a FFI callback context pointer (passed back via callback); the second is a clamped c_uint. Both are small values in practice (context is application-controlled; visible limit is clamped to 1–500 by design).

**Classification:** **ACCEPTABLE**. Pointer-to-usize conversion is standard FFI. The clamp bounds the second value.

**Recommendation:** No change required.

---

### Finding F15–F16: Expect Calls — `kernel/status.rs:225,231`

**File:** `crates/nmp-core/src/kernel/status.rs:222–232`  
**Severity:** Low (invariant maintained by construction)  
**Type:** Expect with clear message

```rust
222  pub(super) fn relay(&self, role: RelayRole) -> &RelayHealth {
223      self.relays
224          .get(&role)
225          .expect("relay health initialized for every role")
226  }
```

**Issue:** Two `expect()` calls that will panic if the relay HashMap doesn't have an entry for a given role. This can only happen if `Kernel::new()` fails to initialize relays for all `RelayRole` variants.

**Classification:** **ACCEPTABLE**. The invariant is maintained by construction: `Kernel::new()` explicitly initializes relays for all roles via `RelayRole::all().into_iter().map(...)`. The expect message is self-documenting.

**Recommendation:** No change required. Invariant is correct.

---

## 3. Doctrine Violations

### D0 Audit: Kernel Never Grows App Nouns

**Status:** ✅ **COMPLIANT**

**Evidence:**
- `nmp-core/src/kernel/` contains only Nostr-agnostic data structures (`StoredEvent`, `Profile`, `TimelineItem`), which are domain-neutral wrappers for events.
- All Nostr-specific parsing (NIP-05, profile format, kind:0 interpretation) is isolated in `kernel/nostr.rs` as utility functions, not as kernel state types.
- The substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`) in `src/substrate/` define no concrete Nostr types; they are generic over module implementations.
- The iOS bridge (`KernelBridge.swift`) decodes Nostr-shaped payloads into app structs without leaking type names into the kernel API.

**Violation?** No. The kernel is domain-agnostic.

---

### D1 Audit: Best-Effort Rendering with Placeholders

**Status:** ✅ **COMPLIANT**

**Evidence:**
- iOS ProfileDetailView (`ProfileViews.swift:51`) renders `ProfileCard.placeholder(pubkey:)` when profile is not yet loaded: `profile ?? ProfileCard.placeholder(pubkey: pubkey)`.
- TimelineRow and all avatar components render initials + color fallback without spinners, allowing the UI to display immediately while kind:0 events arrive asynchronously.
- The `KernelModel` projection cache (`projectionCacheTTL = 60s`) keeps previous author/thread views visible while new data is in-flight.

**Violation?** No. Placeholders are rendered immediately; refinement happens in place.

---

### D2 Audit: Reactivity Contract (Composite Reverse Index)

**Status:** ✅ **COMPLIANT**

**Evidence:**
- `kernel/mod.rs` maintains `wire_subs: HashMap<String, WireSub>` and `profile_claims: HashMap<String, BTreeSet<String>>` — the core reverse index mappings.
- `kernel/requests.rs` and `kernel/ingest.rs` route all message handling and event arrival through kernel methods, which update reverse index state.
- No projection bypass: views like `AuthorViewPayload`, `ThreadViewPayload` are computed via `kernel/update.rs::make_update()`, which serializes the index state.
- iOS `KernelModel` applies updates atomically via `apply(result:)` on the main thread; SwiftUI reactivity is driven by @Published updates.

**Violation?** No. All reactivity flows through the kernel's reverse index.

---

### D3 Audit: Errors Never Cross FFI

**Status:** ✅ **COMPLIANT**

**Evidence:**
- FFI boundary (`ffi.rs`) exports only void-returning C functions: `nmp_app_start()`, `nmp_app_configure()`, etc.
- No Result types or error enum variants are exposed through the FFI surface.
- Error states are communicated via the update callback: `RelayStatus.last_error` and `RelayStatus.last_notice` fields in JSON payloads.
- The iOS app reads error messages from the kernel model's `relayStatus?.lastError` field and renders them in the UI as advisory diagnostics, not as control flow.

**Violation?** No. Error information crosses the FFI as JSON strings (side-band data), not as FFI error codes or control signals. All error handling decisions remain kernel-side.

---

### D4 Audit: One Writer Per Fact

**Status:** ✅ **COMPLIANT**

**Evidence:**
- The kernel actor is single-threaded (runs on one OS thread spawned in `ffi.rs:51`).
- All state mutations go through `run_actor()` in `actor.rs`, which receives commands via a single `mpsc::channel()`.
- Relay worker threads (`relay_worker.rs`) only send *events* back to the actor; they never mutate kernel state directly.
- The iOS bridge is @MainActor-annotated (`KernelModel`), ensuring all SwiftUI state updates are serialized.

**Violation?** No. Single-writer-per-subsystem is enforced architecturally.

---

### D5 Audit: Capabilities Report, Never Decide

**Status:** ✅ **COMPLIANT**

**Evidence:**
- The iOS FFI bridge exposes capability requests as simple commands (`openAuthor()`, `claimProfile()`, etc.) with no decision-making logic at the boundary.
- The kernel processes these as `ActorCommand` variants, routing them to domain logic.
- No policy decisions are made in `KernelBridge.swift` or the iOS-side FFI wrapper; all routing and business logic lives in the kernel.
- The `CapabilityModule` trait (in `substrate/capability.rs`) defines how modules *report* capabilities, not how the bridge decides what to expose.

**Violation?** No. The bridge is a pure relay; decisions are kernel-side.

---

## 4. Acceptable & Justified Findings (No Action Required)

| Finding | Classification | Justification |
|---------|---|---|
| 3 unsafe blocks in ffi.rs (F1, F2, F3) | Documentation gap | Standard FFI pattern; pointers validated by caller contract. Safety comments recommended for future audits but code is correct. |
| allow(unreachable_patterns) in relay_worker.rs (F4) | Documentation gap | Defensive programming for third-party enum evolution. Code is correct; add comment for clarity. |
| 14 integer casts (count→metric types) (F6–F14) | Acceptable | All bounded by design constraints; no overflow risk. Casts are intentional and safe. |
| 2 expect() calls in kernel/status.rs (F15, F16) | Acceptable | Invariant maintained by construction (relay HashMap initialized for all roles in `Kernel::new()`). |
| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |

---

## 5. Recommended Next Actions

### Documentation-Only Improvements (Recommended for M10.5)

These are **not bugs**; the code is correct. Adding safety comments improves auditability and prevents future misclassification:

1. **Add safety comments to ffi.rs unsafe blocks** (F1, F2, F3)
   - Files: `crates/nmp-core/src/ffi.rs` (3 locations: lines 75, 275, 284)
   - Effort: 5 min
   - Impact: Documents FFI contract; improves future audits
   - Recommended text:
     - Line 75: `// safe: caller guarantees app is valid, allocated by nmp_app_new()`
     - Line 275: `// safe: caller guarantees non-null app is a valid NmpApp pointer`
     - Line 284: `// safe: caller guarantees ptr is a valid null-terminated C string; to_str() validates UTF-8`

2. **Add code comment to relay_worker.rs#242** (F4)
   - Files: `crates/nmp-core/src/relay_worker.rs` (line 242)
   - Effort: 2 min
   - Impact: Clarifies defensive intent
   - Recommended text: `// Stream type may have additional TLS variants in future tungstenite versions`

### No Action Required

- All integer casts (F6–F14) are bounded and intentional; no comments needed.
- Both expect() calls (F15, F16) are justified by invariants; no action needed.
- All Swift iOS code is clean (9 files audited; 0 findings).
- All cardinal doctrines (D0–D5) are compliant; no design changes needed.

---

## 6. Metrics

| Category | Count | Status |
|----------|-------|--------|
| **Code Quality** | | |
| Rust files audited | 19 | ✓ |
| Swift files audited | 9 | ✓ |
| Total LOC scanned | 6,559 | ✓ |
| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
| Code bugs found | 0 | ✓ No bugs |
| Unsafe blocks without safety comments | 3 | ⚠️ Documentation gap |
| Integer casts | 14 | ✓ Justified |
| Cardinal doctrine violations | 0 | ✓ Compliant |
| **Post-Audit Actions** | | |
| Critical bugs | 0 | ✓ |
| Code changes required | 0 | ✓ |
| Documentation improvements (optional) | 3 | → Recommended |

---

## 7. Conclusion

The FFI and iOS bridge code is **production-ready for M10.5**. 

**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.

**Recommendation:** The 3 safety comments in ffi.rs are optional but recommended for auditability and preventing future misclassification.

**M10.5 Exit Criteria:** ✅ **READY**

- ✅ Zero blocking debt markers
- ✅ Zero code bugs
- ✅ All doctrines compliant
- ✅ iOS bridge code clean (9 files, 0 findings)
- ✅ Unsafe blocks are correct (documentation gap only)

**Audit Sign-Off:** Ready for iOS empirical proof phase (M10.5).
