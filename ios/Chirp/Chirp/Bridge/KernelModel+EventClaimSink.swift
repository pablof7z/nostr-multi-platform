import Foundation

extension KernelModel: EventClaimSinkProtocol {
    func claim(uri: String, consumerId: String) {
        kernel.claimEvent(uri: uri, consumerID: consumerId)
    }
    func release(uri: String, consumerId: String) {
        kernel.releaseEvent(uri: uri, consumerID: consumerId)
    }
}
