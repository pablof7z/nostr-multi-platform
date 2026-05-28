import FlatBuffers
import Foundation

enum KernelUpdateFrameDecoderError: LocalizedError {
    case emptyPayload
    case missingSnapshotPayload
    case missingPanicPayload
    case unexpectedValueKind(String)

    var errorDescription: String? {
        switch self {
        case .emptyPayload:
            return "empty FlatBuffers update payload"
        case .missingSnapshotPayload:
            return "snapshot frame missing payload"
        case .missingPanicPayload:
            return "panic frame missing payload"
        case let .unexpectedValueKind(kind):
            return "unexpected FlatBuffers value kind \(kind)"
        }
    }
}

enum KernelUpdateFrame {
    case snapshot(UInt32, KernelUpdate, [TypedProjectionEnvelope])
    case panic(String)
}

/// ADR-0037: a typed FlatBuffers sidecar carried alongside the generic
/// `payload` Value tree. Each envelope wraps one named projection's opaque
/// NFTS/NFCT bytes plus its schema identity. Hosts that recognise a `schemaId`
/// decode the bytes with the matching typed decoder; others ignore it and fall
/// back to the generic snapshot.
struct TypedProjectionEnvelope {
    let key: String
    let schemaId: String
    let schemaVersion: UInt32
    let fileIdentifier: String
    let payload: Data
}

enum KernelUpdateFrameDecoder {
    static func decode(_ data: Data) throws -> KernelUpdateFrame {
        guard !data.isEmpty else { throw KernelUpdateFrameDecoderError.emptyPayload }
        var buffer = ByteBuffer(data: data)
        let frame: nmp_transport_UpdateFrame = try getCheckedRoot(
            byteBuffer: &buffer,
            fileId: "NMPU")

        switch frame.kind {
        case .snapshot:
            guard let snapshot = frame.snapshot,
                  let payload = snapshot.payload else {
                throw KernelUpdateFrameDecoderError.missingSnapshotPayload
            }
            let update = try KernelUpdate(from: FlatBufferValueDecoder(value: payload, codingPath: []))
            let typedProjections = extractTypedProjections(from: snapshot)
            return .snapshot(snapshot.schemaVersion, update, typedProjections)
        case .panic:
            guard let message = frame.panic?.msg else {
                throw KernelUpdateFrameDecoderError.missingPanicPayload
            }
            return .panic(message)
        }
    }

    /// ADR-0037: lift the typed projection sidecar into plain Swift envelopes.
    /// Projections missing a key, schema id, or payload table are skipped so a
    /// malformed entry never aborts the whole snapshot.
    private static func extractTypedProjections(
        from snapshot: nmp_transport_SnapshotFrame
    ) -> [TypedProjectionEnvelope] {
        var envelopes: [TypedProjectionEnvelope] = []
        let projections = snapshot.typedProjections
        envelopes.reserveCapacity(projections.count)
        for projection in projections {
            guard let key = projection.key,
                  let typed = projection.payload,
                  let schemaId = typed.schemaId else {
                continue
            }
            envelopes.append(TypedProjectionEnvelope(
                key: key,
                schemaId: schemaId,
                schemaVersion: typed.schemaVersion,
                fileIdentifier: typed.fileIdentifier ?? "",
                payload: Data(typed.payload)
            ))
        }
        return envelopes
    }
}

private final class FlatBufferValueDecoder: Decoder {
    let value: nmp_transport_Value?
    let codingPath: [CodingKey]
    let userInfo: [CodingUserInfoKey: Any] = [:]

    init(value: nmp_transport_Value?, codingPath: [CodingKey]) {
        self.value = value
        self.codingPath = codingPath
    }

    func container<Key>(keyedBy type: Key.Type) throws -> KeyedDecodingContainer<Key> where Key: CodingKey {
        guard let value, value.kind == .map else {
            throw DecodingError.typeMismatch(
                [String: nmp_transport_Value].self,
                DecodingError.Context(codingPath: codingPath, debugDescription: "expected map"))
        }
        return KeyedDecodingContainer(FlatBufferKeyedContainer<Key>(
            pairs: value.map,
            codingPath: codingPath
        ))
    }

    func unkeyedContainer() throws -> UnkeyedDecodingContainer {
        guard let value, value.kind == .list else {
            throw DecodingError.typeMismatch(
                [nmp_transport_Value].self,
                DecodingError.Context(codingPath: codingPath, debugDescription: "expected list"))
        }
        return FlatBufferUnkeyedContainer(values: Array(value.list), codingPath: codingPath)
    }

    func singleValueContainer() throws -> SingleValueDecodingContainer {
        FlatBufferSingleValueContainer(value: value, codingPath: codingPath)
    }
}

private struct FlatBufferKeyedContainer<Key: CodingKey>: KeyedDecodingContainerProtocol {
    let values: [String: nmp_transport_Value?]
    let codingPath: [CodingKey]
    var allKeys: [Key] { values.keys.compactMap(Key.init(stringValue:)) }

    init(pairs: FlatbufferVector<nmp_transport_Pair>, codingPath: [CodingKey]) {
        var values: [String: nmp_transport_Value?] = [:]
        for pair in pairs {
            values[Self.convertFromSnakeCase(pair.key)] = pair.value
        }
        self.values = values
        self.codingPath = codingPath
    }

    func contains(_ key: Key) -> Bool {
        values[key.stringValue] != nil
    }

    func decodeNil(forKey key: Key) throws -> Bool {
        guard let value = values[key.stringValue] else { return true }
        return value?.kind == .null
    }

    func decode<T>(_ type: T.Type, forKey key: Key) throws -> T where T: Decodable {
        try T(from: decoder(forKey: key))
    }

    func nestedContainer<NestedKey>(
        keyedBy type: NestedKey.Type,
        forKey key: Key
    ) throws -> KeyedDecodingContainer<NestedKey> where NestedKey: CodingKey {
        try decoder(forKey: key).container(keyedBy: type)
    }

    func nestedUnkeyedContainer(forKey key: Key) throws -> UnkeyedDecodingContainer {
        try decoder(forKey: key).unkeyedContainer()
    }

    func superDecoder() throws -> Decoder {
        FlatBufferValueDecoder(value: nil, codingPath: codingPath)
    }

    func superDecoder(forKey key: Key) throws -> Decoder {
        try decoder(forKey: key)
    }

    private func decoder(forKey key: Key) throws -> FlatBufferValueDecoder {
        guard let value = values[key.stringValue] else {
            throw DecodingError.keyNotFound(
                key,
                DecodingError.Context(codingPath: codingPath, debugDescription: "missing key \(key.stringValue)"))
        }
        return FlatBufferValueDecoder(value: value, codingPath: codingPath + [key])
    }

    private static func convertFromSnakeCase(_ key: String) -> String {
        guard key.contains("_") else { return key }
        // Match the subset of JSONDecoder.convertFromSnakeCase used by Rust
        // snapshot keys: underscores between words are removed, while leading
        // and trailing underscores are preserved so future private-looking
        // fields cannot alias public names.
        let leading = key.prefix(while: { $0 == "_" })
        var trailingCount = 0
        var cursor = key.endIndex
        while cursor > key.startIndex {
            let previous = key.index(before: cursor)
            guard key[previous] == "_" else { break }
            trailingCount += 1
            cursor = previous
        }
        let trailing = key.suffix(trailingCount)
        let start = key.index(key.startIndex, offsetBy: leading.count)
        let end = key.index(key.endIndex, offsetBy: -trailingCount)
        guard start < end else { return key }
        let body = key[start..<end]
        var result = ""
        var capitalizeNext = false
        for character in body {
            if character == "_" {
                capitalizeNext = !result.isEmpty
                continue
            }
            if capitalizeNext {
                result += String(character).uppercased()
                capitalizeNext = false
            } else {
                result.append(character)
            }
        }
        return String(leading) + result + String(trailing)
    }
}

private struct FlatBufferUnkeyedContainer: UnkeyedDecodingContainer {
    let values: [nmp_transport_Value]
    let codingPath: [CodingKey]
    var currentIndex = 0
    var count: Int? { values.count }
    var isAtEnd: Bool { currentIndex >= values.count }

    mutating func decodeNil() throws -> Bool {
        guard !isAtEnd else { throw endOfContainer() }
        if values[currentIndex].kind == .null {
            currentIndex += 1
            return true
        }
        return false
    }

    mutating func decode<T>(_ type: T.Type) throws -> T where T: Decodable {
        guard !isAtEnd else { throw endOfContainer() }
        defer { currentIndex += 1 }
        return try T(from: FlatBufferValueDecoder(value: values[currentIndex], codingPath: codingPath))
    }

    mutating func nestedContainer<NestedKey>(
        keyedBy type: NestedKey.Type
    ) throws -> KeyedDecodingContainer<NestedKey> where NestedKey: CodingKey {
        guard !isAtEnd else { throw endOfContainer() }
        defer { currentIndex += 1 }
        return try FlatBufferValueDecoder(
            value: values[currentIndex],
            codingPath: codingPath
        ).container(keyedBy: type)
    }

    mutating func nestedUnkeyedContainer() throws -> UnkeyedDecodingContainer {
        guard !isAtEnd else { throw endOfContainer() }
        defer { currentIndex += 1 }
        return try FlatBufferValueDecoder(value: values[currentIndex], codingPath: codingPath)
            .unkeyedContainer()
    }

    mutating func superDecoder() throws -> Decoder {
        guard !isAtEnd else { throw endOfContainer() }
        defer { currentIndex += 1 }
        return FlatBufferValueDecoder(value: values[currentIndex], codingPath: codingPath)
    }

    private func endOfContainer() -> DecodingError {
        DecodingError.valueNotFound(
            nmp_transport_Value.self,
            DecodingError.Context(codingPath: codingPath, debugDescription: "unkeyed container is at end"))
    }
}

private struct FlatBufferSingleValueContainer: SingleValueDecodingContainer {
    let value: nmp_transport_Value?
    let codingPath: [CodingKey]

    func decodeNil() -> Bool {
        guard let value else { return true }
        return value.kind == .null
    }

    func decode(_ type: Bool.Type) throws -> Bool {
        guard let value, value.kind == .bool else { throw mismatch(type) }
        return value.boolValue
    }

    func decode(_ type: String.Type) throws -> String {
        guard let value, value.kind == .string else { throw mismatch(type) }
        return value.stringValue ?? ""
    }

    func decode(_ type: Double.Type) throws -> Double {
        guard let value else { throw mismatch(type) }
        switch value.kind {
        case .float:
            return value.floatValue
        case .int:
            return Double(value.intValue)
        case .uint:
            return Double(value.uintValue)
        default:
            throw mismatch(type)
        }
    }

    func decode(_ type: Float.Type) throws -> Float {
        Float(try decode(Double.self))
    }

    func decode(_ type: Int.Type) throws -> Int { try signedInteger(type) }
    func decode(_ type: Int8.Type) throws -> Int8 { try signedInteger(type) }
    func decode(_ type: Int16.Type) throws -> Int16 { try signedInteger(type) }
    func decode(_ type: Int32.Type) throws -> Int32 { try signedInteger(type) }
    func decode(_ type: Int64.Type) throws -> Int64 { try signedInteger(type) }
    func decode(_ type: UInt.Type) throws -> UInt { try unsignedInteger(type) }
    func decode(_ type: UInt8.Type) throws -> UInt8 { try unsignedInteger(type) }
    func decode(_ type: UInt16.Type) throws -> UInt16 { try unsignedInteger(type) }
    func decode(_ type: UInt32.Type) throws -> UInt32 { try unsignedInteger(type) }
    func decode(_ type: UInt64.Type) throws -> UInt64 { try unsignedInteger(type) }

    func decode<T>(_ type: T.Type) throws -> T where T: Decodable {
        try T(from: FlatBufferValueDecoder(value: value, codingPath: codingPath))
    }

    private func signedInteger<T: FixedWidthInteger & SignedInteger>(_ type: T.Type) throws -> T {
        guard let value else { throw mismatch(type) }
        switch value.kind {
        case .int:
            guard let converted = T(exactly: value.intValue) else { throw mismatch(type) }
            return converted
        case .uint:
            guard let converted = T(exactly: value.uintValue) else { throw mismatch(type) }
            return converted
        default:
            throw mismatch(type)
        }
    }

    private func unsignedInteger<T: FixedWidthInteger & UnsignedInteger>(_ type: T.Type) throws -> T {
        guard let value else { throw mismatch(type) }
        switch value.kind {
        case .uint:
            guard let converted = T(exactly: value.uintValue) else { throw mismatch(type) }
            return converted
        case .int:
            guard let converted = T(exactly: value.intValue) else { throw mismatch(type) }
            return converted
        default:
            throw mismatch(type)
        }
    }

    private func mismatch<T>(_ type: T.Type) -> DecodingError {
        DecodingError.typeMismatch(
            type,
            DecodingError.Context(codingPath: codingPath, debugDescription: "FlatBuffers value type mismatch"))
    }
}
