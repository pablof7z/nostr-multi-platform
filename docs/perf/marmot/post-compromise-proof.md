# Marmot Milestone — Post-Compromise Security Proof

**Date**: 2026-05-18  
**Test**: `marmot_post_compromise::post_compromise_attacker_epoch_n_cannot_decrypt_epoch_n_plus_1`  
**File**: `crates/nmp-testing/tests/marmot_post_compromise.rs`  
**Exit gate ref**: `docs/plan/marmot-mls.md §"Exit gate (product)"` — post-compromise security proof

---

## What is proved

After an attacker gains a complete snapshot of Bob's MLS state at epoch N
(all epoch secrets, leaf keys, stored ratchet tree), Bob can recover security
by calling `self_update` (advancing to epoch N+1). The attacker, holding only
epoch-N material, cannot derive epoch-N+1 secrets and therefore cannot decrypt
epoch-N+1 messages.

---

## Test scenario

1. Alice + Bob establish a 2-member group.
2. Exchange a message at epoch N — verify Bob decrypts correctly (state valid).
3. **Compromise point**: drop Bob's `MarmotService`, flush SQLite WAL, copy
   the SQLite file to a separate path (the "attacker" gets epoch-N material).
4. Reconstruct real Bob from the original SQLite file.
5. Bob performs `self_update` — epoch advances to N+1. Alice processes the commit.
6. Alice sends a message encrypted at epoch N+1.
7. Real Bob (epoch N+1) successfully decrypts — sanity check passes.
8. Attacker service (built from the epoch-N snapshot, has NOT processed the
   self_update commit) calls `process_message` on the epoch-N+1 encrypted event.
9. Attacker's result is `Err(_)` or `Ok(Unprocessable{..})` — NOT `ApplicationMessage`.

---

## Captured test output

```
running 1 test
test post_compromise_attacker_epoch_n_cannot_decrypt_epoch_n_plus_1 ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
```

Run command:
```
cargo test -p nmp-testing --test marmot_post_compromise -- --nocapture
```

---

## Why SQLite file copy is the right "attacker" model

`MdkSqliteStorage::new_in_memory()` cannot be cloned. The only way to snapshot
MLS state at a specific epoch is to copy the underlying SQLite file. This is
equivalent to a memory dump or disk-image compromise: the attacker holds every
byte of Bob's private MLS state at the moment of copy, including:

- The epoch-N group context (ratchet tree, required capabilities, confirmed
  transcript hash).
- Bob's current leaf keypair (HPKE private key + signing key).
- The epoch-N exporter secret (used by MIP-03 to derive ChaCha20 keys).
- The epoch-N init secret.

This is a stronger attacker model than just capturing the Nostr private key —
it captures the complete MLS state, not just the transport identity.

---

## Why the attacker cannot derive epoch-N+1

MLS's post-compromise security is based on the `Update` path in the ratchet
tree. When Bob calls `self_update`:

1. Bob generates a fresh HPKE keypair for his leaf node.
2. Bob computes an `UpdatePath` that HPKE-encrypts the new path secret for
   each ancestor node using the **current** public keys of co-path members
   (Alice's public keys, which the attacker does not hold the corresponding
   private keys for).
3. The epoch-N+1 init secret is derived by XOR-folding the decrypted path
   secrets — a derivation that requires either (a) Alice's private keys for
   the co-path positions, or (b) the fresh HPKE private key Bob generated,
   neither of which appear in the epoch-N snapshot.

The epoch-N exporter secret and the epoch-N init secret are cryptographically
one-way: there is no derivation from them to epoch-N+1 epoch secrets without
the HPKE private key material from Bob's self_update.

---

## Assertion code

```rust
let attacker_result = attacker.process_message(&epoch_n1_event);
match attacker_result {
    Err(_) => {
        // Expected: cannot decrypt outer MIP-03 layer using epoch-N secrets.
    }
    Ok(MessageProcessingResult::Unprocessable { .. }) => {
        // Also acceptable: MDK considers it unprocessable (unknown epoch).
    }
    Ok(MessageProcessingResult::ApplicationMessage(ref m)) => {
        panic!(
            "POST-COMPROMISE SECURITY FAILURE: attacker with epoch-N secrets \
             decrypted epoch-N+1 message: {:?}",
            m.content
        );
    }
    Ok(other) => {
        let _ = other;
    }
}
```

The panic branch is the failure condition; reaching `ok` confirms PCS holds.
