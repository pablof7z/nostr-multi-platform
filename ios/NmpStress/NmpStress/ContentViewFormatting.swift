import Foundation

extension ContentView {
    func format(_ value: UInt64?) -> String {
        guard let value else { return "0" }
        return value.formatted()
    }

    func format(_ value: Int?) -> String {
        guard let value else { return "0" }
        return value.formatted()
    }

    func bytes(_ value: Int?) -> String {
        guard let value else { return "0 B" }
        return ByteCountFormatter.string(fromByteCount: Int64(value), countStyle: .memory)
    }

    func bytes(_ value: UInt64?) -> String {
        guard let value else { return "0 B" }
        return ByteCountFormatter.string(fromByteCount: Int64(value), countStyle: .memory)
    }

    func millis(_ value: UInt64?) -> String {
        guard let value else { return "-" }
        return "\(value)ms"
    }
}
