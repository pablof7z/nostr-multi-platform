import Foundation

// MARK: - String whitespace helpers

extension String {
    /// Returns the string with leading and trailing whitespace and newlines removed.
    var trimmed: String {
        trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Returns `true` when the string contains only whitespace and newlines
    /// (i.e. `trimmed.isEmpty`).
    var isBlank: Bool {
        trimmed.isEmpty
    }
}

// MARK: - Optional<String> helpers

extension Optional where Wrapped == String {
    /// Returns the trimmed string, or `""` when `nil`.
    var trimmedOrEmpty: String {
        self?.trimmed ?? ""
    }
}
