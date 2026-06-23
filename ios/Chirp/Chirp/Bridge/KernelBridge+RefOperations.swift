import Foundation

// ── Reference resolution / event claim operations ────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    /// ADR-0063 Lane E (#1671) — unified, origin-blind reference resolution.
    /// Supersedes `claimProfile` / `claimEvent`: registers (or upgrades) this
    /// `consumerID`'s interest in `(namespace, key)` at the requested `shape`
    /// and `liveness`. The kernel surfaces the resolved entity in the matching
    /// keyed projection (`refs.profile` / `refs.event`) keyed by `key`, which
    /// the `keyedRefCache` consumes on the next frame. Fire-and-forget.
    func resolveRef(
        namespace: RefNamespace,
        key: String,
        consumerID: String,
        shape: RefShape,
        liveness: RefLiveness
    ) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_resolve_ref(
                    raw, namespace.rawValue, keyPtr, cidPtr,
                    shape.rawValue, liveness.rawValue)
            }
        }
    }

    /// ADR-0063 Lane E (#1671) — release a reference registered via
    /// `resolveRef`. Pass the SAME `namespace` / `key` / `consumerID`.
    func releaseRef(namespace: RefNamespace, key: String, consumerID: String) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_ref(raw, namespace.rawValue, keyPtr, cidPtr)
            }
        }
    }

    /// ADR-0032 / V-115: bech32-encode a hex pubkey as `npub1…` on the shell
    /// side. Projections no longer carry pre-encoded npub strings; shells call
    /// this when they need the bech32 form (copy-to-clipboard, share sheet).
    /// Returns `nil` if the C function fails (e.g. invalid key).
    func encodeProfile(pubkey: String) -> String? {
        pubkey.withCString { pkPtr -> String? in
            guard let ptr = nmp_app_encode_profile(raw, pkPtr) else { return nil }
            defer { nmp_free_string(ptr) }
            return String(cString: ptr)
        }
    }

    /// F-TTL — `force` controls the lazy re-verification gate; it only has an
    /// effect for `naddr` (addressable / replaceable) URIs and is a silent
    /// no-op for immutable `nevent`/`note` URIs. Pass `true` only when the
    /// user explicitly navigated to / opened this article/event or pulled to
    /// refresh; default `false` is the background path.
    func claimEvent(uri: String, consumerID: String, force: Bool = false) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_event(raw, uriPtr, cidPtr, force ? 1 : 0)
            }
        }
    }

    func releaseEvent(uri: String, consumerID: String) {
        uri.withCString { uriPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_event(raw, uriPtr, cidPtr)
            }
        }
    }
}
