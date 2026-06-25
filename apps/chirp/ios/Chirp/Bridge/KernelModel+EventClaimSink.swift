import Foundation

// App-owned URI adapter: claimEventUri/releaseEventUri decode the nostr: URI
// and forward to the typed event-ref FFI adapters.
extension KernelModel: EventClaimSinkProtocol {
    func claim(uri: String, consumerId: String) {
        kernel.claimEventUri(uri: uri, consumerID: consumerId)
    }
    func release(uri: String, consumerId: String) {
        kernel.releaseEventUri(uri: uri, consumerID: consumerId)
    }
}
