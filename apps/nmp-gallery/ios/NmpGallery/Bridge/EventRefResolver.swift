import Foundation

/// Concrete `EventRefResolverProtocol` impl that forwards renderer-driven
/// event-ref lifecycle requests to the gallery's kernel actor via
/// `GalleryKernelHandle`.
///
/// The renderer (`NostrContentView` / `EmbeddedEvent`) calls `resolveEventRef`
/// exactly once per URI on `.task(id:)` and the matching `releaseEventRef` on
/// `.onDisappear`.
/// Both methods are fire-and-forget at the FFI boundary — the kernel actor
/// owns the refcounted interest table — so the sink is safely `Sendable`
/// even though it captures a non-Sendable `GalleryKernelHandle` reference:
/// the handle's `raw` pointer is the actor's identity, never accessed
/// directly from this type.
final class KernelEventRefResolver: EventRefResolverProtocol, @unchecked Sendable {
    private let kernel: GalleryKernelHandle

    init(kernel: GalleryKernelHandle) {
        self.kernel = kernel
    }

    // #1726: routed through resolveEventRef/releaseEventRef, which decode the nostr:
    // URI and forward to typed event-ref FFI adapters.
    func resolveEventRef(uri: String, consumerId: String) {
        kernel.resolveEventRef(uri: uri, consumerID: consumerId)
    }

    func releaseEventRef(uri: String, consumerId: String) {
        kernel.releaseEventRef(uri: uri, consumerID: consumerId)
    }
}
