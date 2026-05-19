import Foundation

// Bech32 encoding used by Nostr (npub/nsec) — no checksum variant (Bech32, not Bech32m).

enum Bech32 {
    private static let charset = Array("qpzry9x8gf2tvdw0s3jn54khce6mua7l")
    private static let generator: [UInt32] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
    /// Number of 5-bit checksum characters appended to every Bech32 string.
    private static let checksumLength = 6

    static func encode(hrp: String, data: Data) -> String {
        // Force-unwrap is safe: convertBits returns nil only when pad==false and trailing bits
        // are non-zero; with pad==true it always appends the remaining bits and returns a value.
        let values = convertBits(data: Array(data), fromBits: 8, toBits: 5, pad: true)!
        let checksum = createChecksum(hrp: hrp, data: values)
        let combined = values + checksum
        let encoded = combined.map { charset[Int($0)] }
        return hrp + "1" + String(encoded)
    }

    static func decode(_ str: String) -> (hrp: String, data: Data)? {
        let lower = str.lowercased()
        guard let separatorIdx = lower.lastIndex(of: "1") else { return nil }
        let hrp = String(lower[..<separatorIdx])
        let encoded = String(lower[lower.index(after: separatorIdx)...])
        guard !hrp.isEmpty, encoded.count >= checksumLength else { return nil }

        var decoded: [UInt8] = []
        for ch in encoded {
            guard let idx = charset.firstIndex(of: ch) else { return nil }
            decoded.append(UInt8(idx))
        }
        guard verifyChecksum(hrp: hrp, data: decoded) else { return nil }
        let payload = Array(decoded.dropLast(checksumLength))
        guard let bytes = convertBits(data: payload, fromBits: 5, toBits: 8, pad: false) else { return nil }
        return (hrp, Data(bytes))
    }

    private static func convertBits(data: [UInt8], fromBits: Int, toBits: Int, pad: Bool) -> [UInt8]? {
        var acc = 0
        var bits = 0
        var result: [UInt8] = []
        let maxv = (1 << toBits) - 1
        for value in data {
            acc = (acc << fromBits) | Int(value)
            bits += fromBits
            while bits >= toBits {
                bits -= toBits
                result.append(UInt8((acc >> bits) & maxv))
            }
        }
        if pad {
            if bits > 0 { result.append(UInt8((acc << (toBits - bits)) & maxv)) }
        } else if bits >= fromBits || ((acc << (toBits - bits)) & maxv) != 0 {
            return nil
        }
        return result
    }

    private static func polymod(_ values: [UInt8]) -> UInt32 {
        var chk: UInt32 = 1
        for v in values {
            let top = chk >> 25
            chk = (chk & 0x1ffffff) << 5 ^ UInt32(v)
            for i in 0..<5 {
                chk ^= (top >> i) & 1 == 1 ? generator[i] : 0
            }
        }
        return chk
    }

    private static func hrpExpand(_ hrp: String) -> [UInt8] {
        let hrpBytes = hrp.unicodeScalars.map { UInt8($0.value) }
        return hrpBytes.map { $0 >> 5 } + [0] + hrpBytes.map { $0 & 31 }
    }

    private static func createChecksum(hrp: String, data: [UInt8]) -> [UInt8] {
        let values = hrpExpand(hrp) + data + [UInt8](repeating: 0, count: checksumLength)
        let polymod = Self.polymod(values) ^ 1
        return (0..<checksumLength).map { UInt8((polymod >> (5 * (checksumLength - 1 - $0))) & 31) }
    }

    private static func verifyChecksum(hrp: String, data: [UInt8]) -> Bool {
        polymod(hrpExpand(hrp) + data) == 1
    }
}
