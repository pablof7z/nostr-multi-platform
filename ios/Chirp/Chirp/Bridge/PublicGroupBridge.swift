import Foundation
import os.log

private let pgLog = Logger(subsystem: "io.f7z.chirp", category: "PublicGroupBridge")

extension KernelHandle {
    @discardableResult
    func createPublicGroup(group: GroupId, name: String, about: String?) -> DispatchResult {
        var payload: [String: Any] = [
            "group": group.jsonObject,
            "name": name,
        ]
        if let about, !about.isEmpty {
            payload["about"] = about
        }
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            pgLog.error("createPublicGroup: failed to encode action payload")
            return .failure("failed to encode public group create payload")
        }
        return dispatchRawAction(
            namespace: "nmp.nip29.create_public_group",
            bodyJson: json)
    }
}
