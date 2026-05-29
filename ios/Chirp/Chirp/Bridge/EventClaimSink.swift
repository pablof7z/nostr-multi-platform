import Foundation

/// Protocol mirror of nmp_content::EventClaimSink.
/// Components fire claim(uri:consumerId:) when an embed enters the view tree
/// and the matching release when it leaves. KernelModel conforms.
@MainActor
protocol EventClaimSinkProtocol: Sendable {
    func claim(uri: String, consumerId: String)
    func release(uri: String, consumerId: String)
}
