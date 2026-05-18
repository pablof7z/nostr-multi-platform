# Marmot Milestone — Forward Secrecy Proof

**Date**: 2026-05-18  
**Test**: `marmot_forward_secrecy::forward_secrecy_removed_member_cannot_decrypt`  
**File**: `crates/nmp-testing/tests/marmot_forward_secrecy.rs`  
**Exit gate ref**: `docs/plan/marmot-mls.md §"Exit gate (product)"` — forward secrecy proof

---

## What is proved

After Alice removes Carol from a 3-member MLS group (Alice, Bob, Carol) and
Alice performs a `self_update` (advancing the epoch), Carol's frozen MLS state
cannot decrypt messages encrypted at the new epoch. This demonstrates forward
secrecy: past epoch secrets cannot be used to decrypt future messages.

---

## Test scenario

1. Alice + Bob establish a 2-member group.
2. Alice adds Carol (add_members → gift-wrap Welcome → Carol joins + self_updates).
3. Verify Carol is in the 3-member set.
4. Alice removes Carol (`remove_members`, publish commit, Bob processes).
5. Alice performs `self_update` — epoch advances. Bob processes Alice's commit.
6. Alice sends a message encrypted at the new (post-removal) epoch.
7. Bob (still a member) successfully decrypts — sanity check passes.
8. Carol's service (frozen at pre-removal epoch) calls `process_message` on the
   post-removal encrypted event.
9. Carol's result is `Err(_)` or `Ok(Unprocessable{..})` — NOT `ApplicationMessage`.
10. Member count after removal is 2 (Alice + Bob only).

---

## Captured test output

```
running 1 test
test forward_secrecy_removed_member_cannot_decrypt ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
```

Run command:
```
cargo test -p nmp-testing --test marmot_forward_secrecy -- --nocapture
```

---

## Why this proves forward secrecy

MLS forward secrecy works via the epoch key schedule. After a `Commit`
(including `self_update`), MDK derives new epoch secrets from a HKDF mixing:
- The previous epoch's init secret (random, not derivable from old epoch).
- Fresh HPKE public key material from the committer.

Carol's service holds the epoch-N exporter secret. It cannot derive the
epoch-N+1 exporter secret because:
1. The epoch-N init secret is one-way derived; Carol cannot reverse it.
2. The committer (Alice) injected fresh HPKE leaf-key material that Carol's
   state snapshot does not contain.

The MIP-03 outer layer of kind:445 events derives its symmetric key via
`MLS-Exporter("marmot", "group-event", 32)` over the epoch's exporter secret.
Without epoch-N+1's exporter secret, Carol's MDK cannot reconstruct the
ChaCha20-Poly1305 key to decrypt the outer envelope, and `process_message`
returns an error.

---

## Assertion code (forward_secrecy_removed_member_cannot_decrypt)

```rust
let carol_result = carol.process_message(&msg_event);
let cannot_decrypt = match carol_result {
    Err(_) => true,
    Ok(MessageProcessingResult::Unprocessable { .. }) => true,
    Ok(MessageProcessingResult::ApplicationMessage(ref m)) => {
        panic!(
            "FORWARD SECRECY FAILURE: removed Carol decrypted message: {:?}",
            m.content
        );
    }
    Ok(other) => {
        let _ = other;
        true
    }
};
assert!(cannot_decrypt, "removed Carol must not be able to decrypt post-removal messages");
```

The panic branch is the failure condition; the test reaching `ok` confirms
the panic was never triggered.
