import Foundation

/// Concrete `EventClaimSinkProtocol` impl that forwards renderer-driven
/// claims to the gallery's kernel actor via `GalleryKernelHandle`.
///
/// The renderer (`NostrContentView` / `EmbeddedEvent`) calls `claim` exactly
/// once per URI on `.task(id:)` and the matching `release` on `.onDisappear`.
/// Both methods are fire-and-forget at the FFI boundary — the kernel actor
/// owns the refcounted interest table — so the sink is safely `Sendable`
/// even though it captures a non-Sendable `GalleryKernelHandle` reference:
/// the handle's `raw` pointer is the actor's identity, never accessed
/// directly from this type.
final class KernelEventClaimSink: EventClaimSinkProtocol, @unchecked Sendable {
    private let kernel: GalleryKernelHandle

    init(kernel: GalleryKernelHandle) {
        self.kernel = kernel
    }

    func claim(uri: String, consumerId: String) {
        kernel.claimEvent(uri: uri, consumerID: consumerId)
    }

    func release(uri: String, consumerId: String) {
        kernel.releaseEvent(uri: uri, consumerID: consumerId)
    }
}
