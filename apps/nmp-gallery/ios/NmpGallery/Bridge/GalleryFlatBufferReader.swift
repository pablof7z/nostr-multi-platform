import Foundation

enum GalleryFlatBufferReadError: Error {
    case outOfRange
}

struct GalleryFlatBufferReader {
    let data: Data

    func hasIdentifier(_ identifier: String) -> Bool {
        guard data.count >= 8, identifier.utf8.count == 4 else { return false }
        return Array(identifier.utf8).enumerated().allSatisfy { idx, byte in
            data[4 + idx] == byte
        }
    }

    func rootTable() throws -> Int {
        try indirect(at: 0)
    }

    func tableField(table: Int, index: Int) throws -> Int? {
        guard let field = try field(table: table, index: index) else { return nil }
        return try indirect(at: field)
    }

    func stringField(table: Int, index: Int) throws -> String? {
        guard let field = try field(table: table, index: index) else { return nil }
        return try string(at: indirect(at: field))
    }

    func tableVectorField(table: Int, index: Int) throws -> [Int] {
        guard let field = try field(table: table, index: index) else { return [] }
        let vector = try indirect(at: field)
        let count = Int(try u32(at: vector))
        return try (0..<count).map { item in
            try indirect(at: vector + 4 + item * 4)
        }
    }

    func stringVectorField(table: Int, index: Int) throws -> [String] {
        guard let field = try field(table: table, index: index) else { return [] }
        let vector = try indirect(at: field)
        let count = Int(try u32(at: vector))
        return try (0..<count).compactMap { item in
            try string(at: indirect(at: vector + 4 + item * 4))
        }
    }

    func u32VectorField(table: Int, index: Int) throws -> [UInt32] {
        guard let field = try field(table: table, index: index) else { return [] }
        let vector = try indirect(at: field)
        let count = Int(try u32(at: vector))
        return try (0..<count).map { item in
            try u32(at: vector + 4 + item * 4)
        }
    }

    func bytesVectorField(table: Int, index: Int) throws -> Data {
        guard let field = try field(table: table, index: index) else { return Data() }
        let vector = try indirect(at: field)
        let count = Int(try u32(at: vector))
        let start = vector + 4
        try range(start, count: count)
        return Data(data[start..<start + count])
    }

    func u8Field(table: Int, index: Int) throws -> UInt8? {
        guard let field = try field(table: table, index: index) else { return nil }
        try range(field, count: 1)
        return data[field]
    }

    func boolField(table: Int, index: Int) throws -> Bool? {
        guard let value = try u8Field(table: table, index: index) else { return nil }
        return value != 0
    }

    func u32Field(table: Int, index: Int) throws -> UInt32? {
        guard let field = try field(table: table, index: index) else { return nil }
        return try u32(at: field)
    }

    func u64Field(table: Int, index: Int) throws -> UInt64? {
        guard let field = try field(table: table, index: index) else { return nil }
        return try u64(at: field)
    }

    func i64Field(table: Int, index: Int) throws -> Int64? {
        guard let field = try field(table: table, index: index) else { return nil }
        return Int64(bitPattern: try u64(at: field))
    }

    private func field(table: Int, index: Int) throws -> Int? {
        try range(table, count: 4)
        let vtable = table - Int(try i32(at: table))
        try range(vtable, count: 4)
        let length = Int(try u16(at: vtable))
        let entry = vtable + 4 + index * 2
        guard entry + 2 <= vtable + length else { return nil }
        let offset = Int(try u16(at: entry))
        return offset == 0 ? nil : table + offset
    }

    private func indirect(at offset: Int) throws -> Int {
        offset + Int(try u32(at: offset))
    }

    private func string(at offset: Int) throws -> String? {
        let length = Int(try u32(at: offset))
        let start = offset + 4
        try range(start, count: length)
        return String(data: data[start..<start + length], encoding: .utf8)
    }

    private func u32(at offset: Int) throws -> UInt32 {
        try range(offset, count: 4)
        return UInt32(data[offset])
            | (UInt32(data[offset + 1]) << 8)
            | (UInt32(data[offset + 2]) << 16)
            | (UInt32(data[offset + 3]) << 24)
    }

    private func u16(at offset: Int) throws -> UInt16 {
        try range(offset, count: 2)
        return UInt16(data[offset]) | (UInt16(data[offset + 1]) << 8)
    }

    private func i32(at offset: Int) throws -> Int32 {
        Int32(bitPattern: try u32(at: offset))
    }

    private func u64(at offset: Int) throws -> UInt64 {
        try range(offset, count: 8)
        var value: UInt64 = 0
        for byte in 0..<8 {
            value |= UInt64(data[offset + byte]) << (byte * 8)
        }
        return value
    }

    private func range(_ offset: Int, count: Int) throws {
        guard offset >= 0, count >= 0, offset + count <= data.count else {
            throw GalleryFlatBufferReadError.outOfRange
        }
    }
}
