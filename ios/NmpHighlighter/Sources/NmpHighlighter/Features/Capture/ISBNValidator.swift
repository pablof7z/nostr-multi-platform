import Foundation

/// Validates an EAN-13 payload as an ISBN-13 (Bookland). A grocery or
/// general-product EAN-13 passes the same checksum rule but is not a book —
/// the barcode scanner uses `validate(_:)` to silently reject those rather
/// than launch a catalog lookup on a banana.
///
/// Reference: ISBNs are EAN-13 codes starting with `978` or `979`.
enum ISBNValidator {
    /// Returns the normalized 13-digit ISBN on success, or nil when the input
    /// isn't a valid Bookland EAN-13. Strips whitespace and hyphens; accepts
    /// 10-digit ISBNs (including a trailing `X` check digit) by converting
    /// them to ISBN-13.
    static func validate(_ raw: String) -> String? {
        let digits = raw.filter { $0 != "-" && !$0.isWhitespace }

        if digits.count == 13,
           digits.allSatisfy(\.isASCIIDigit),
           digits.hasPrefix("978") || digits.hasPrefix("979"),
           isValidChecksum13(digits) {
            return digits
        }

        if digits.count == 10, isValidISBN10(digits) {
            return isbn10To13(digits)
        }

        return nil
    }

    private static func isValidChecksum13(_ digits: String) -> Bool {
        guard digits.count == 13 else { return false }
        var sum = 0
        for (i, char) in digits.enumerated() {
            guard let d = char.wholeNumberValue else { return false }
            sum += (i % 2 == 0) ? d : d * 3
        }
        return sum % 10 == 0
    }

    private static func isValidISBN10(_ digits: String) -> Bool {
        guard digits.count == 10 else { return false }
        var sum = 0
        for (i, char) in digits.enumerated() {
            let value: Int
            if char == "X" || char == "x" {
                guard i == 9 else { return false }
                value = 10
            } else if let d = char.wholeNumberValue {
                value = d
            } else {
                return false
            }
            sum += value * (10 - i)
        }
        return sum % 11 == 0
    }

    private static func isbn10To13(_ isbn10: String) -> String {
        let prefix = "978" + String(isbn10.prefix(9))
        let check = computeChecksum13(prefix)
        return prefix + String(check)
    }

    private static func computeChecksum13(_ first12: String) -> Int {
        var sum = 0
        for (i, char) in first12.enumerated() {
            guard let d = char.wholeNumberValue else { return 0 }
            sum += (i % 2 == 0) ? d : d * 3
        }
        return (10 - (sum % 10)) % 10
    }
}

private extension Character {
    var isASCIIDigit: Bool { ("0"..."9").contains(self) }
}
