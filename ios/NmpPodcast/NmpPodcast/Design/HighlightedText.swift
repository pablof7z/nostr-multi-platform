import SwiftUI

// MARK: - HighlightedText

/// Renders `text` with each case-insensitive occurrence of `query` bolded.
struct HighlightedText: View {
    let text: String
    let query: String

    var body: some View {
        Text(Self.makeAttributed(text: text, query: query))
    }

    /// Builds the highlighted AttributedString. Exposed at file scope so
    /// test cases can lock the highlight ranges directly.
    ///
    /// **Bug history.** A previous implementation lowercased both `text`
    /// and `query` and searched the lowercased copy, then walked
    /// grapheme-offsets back into the original. That broke when
    /// `.lowercased()` mutated grapheme-cluster counts (Turkish "İ" →
    /// "i̇" expands one cluster to two), and it missed Unicode-folded
    /// matches like "ß" ↔ "SS" because the literal-string compare on
    /// the lowercased pair didn't apply Foundation's case-insensitive
    /// fold rules. Now we search the original text with
    /// `.caseInsensitive` (Unicode-aware) and only walk graphemes in
    /// the original — the grapheme structure of `out` matches `text`,
    /// so the offsets are stable.
    static func makeAttributed(text: String, query: String) -> AttributedString {
        var out = AttributedString(text)
        let trimmed = query.trimmed
        guard !trimmed.isEmpty else { return out }

        var cursor = text.startIndex
        while cursor < text.endIndex,
              let range = text.range(
                  of: trimmed,
                  options: .caseInsensitive,
                  range: cursor..<text.endIndex
              )
        {
            let lo = text.distance(from: text.startIndex, to: range.lowerBound)
            let hi = text.distance(from: text.startIndex, to: range.upperBound)
            if let s = attributedIndex(out, out.startIndex, offsetBy: lo),
               let e = attributedIndex(out, out.startIndex, offsetBy: hi) {
                out[s..<e].font = .body.bold()
                out[s..<e].foregroundColor = .accentColor
            }
            cursor = range.upperBound
        }
        return out
    }

    private static func attributedIndex(
        _ string: AttributedString,
        _ base: AttributedString.Index,
        offsetBy n: Int
    ) -> AttributedString.Index? {
        var idx = base
        for _ in 0..<n {
            guard idx < string.endIndex else { return nil }
            idx = string.characters.index(after: idx)
        }
        return idx
    }
}
