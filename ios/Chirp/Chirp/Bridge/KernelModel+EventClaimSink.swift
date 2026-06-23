import Foundation

// #1726: migrated from the deleted claimEvent/releaseEvent (nmp_app_claim_event /
// nmp_app_release_event) to claimEventUri/releaseEventUri, which decode the nostr:
// URI and forward to nmp_app_resolve_ref(namespace=event) / nmp_app_release_ref.
extension KernelModel: EventClaimSinkProtocol {
    func claim(uri: String, consumerId: String) {
        kernel.claimEventUri(uri: uri, consumerID: consumerId)
    }
    func release(uri: String, consumerId: String) {
        kernel.releaseEventUri(uri: uri, consumerID: consumerId)
    }
}
