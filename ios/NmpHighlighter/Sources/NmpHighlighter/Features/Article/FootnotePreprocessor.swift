import Foundation

/// GitHub-flavored-Markdown footnote pre-pass. `swift-markdown` implements
/// CommonMark without footnotes, so we lift `[^id]: definition` lines out of
/// the body before handing content to the parser and track them separately.
///
/// Inline `[^id]` references stay in the body as-is; the renderer detects the
/// pattern inside text runs and emits superscript, tappable attributed runs.
enum FootnotePreprocessor {
    struct Output {
        /// Source with definition lines removed.
        let cleanedMarkdown: String
        /// Definitions keyed by id, in source order.
        let definitions: [Definition]
    }

    struct Definition {
        /// The identifier between the brackets (e.g. `1`, `note-alpha`).
        let id: String
        /// Human-facing number starting at 1, in the order the definition
        /// first appears in the source.
        let number: Int
        /// Markdown body of the footnote (may be multi-line, continuations
        /// indented).
        let markdown: String
    }

    /// Split the input into the body used for primary rendering and an
    /// ordered list of footnote definitions.
    ///
    /// Supported syntax (matches GFM):
    ///
    /// ```
    /// Some body text[^1].
    ///
    /// [^1]: The definition. Continuation lines are indented
    ///     four spaces.
    /// ```
    static func extract(_ source: String) -> Output {
        var definitions: [String: (order: Int, lines: [String])] = [:]
        var definitionOrder: [String] = []
        var cleanedLines: [String] = []

        // Current open definition we're collecting continuation lines into.
        // Non-nil between the `[^id]: …` header and the next blank line.
        var currentDefinitionId: String?

        let lines = source.components(separatedBy: "\n")
        for line in lines {
            if let match = parseDefinitionHeader(line) {
                currentDefinitionId = match.id
                if definitions[match.id] == nil {
                    definitions[match.id] = (order: definitionOrder.count, lines: [match.firstLine])
                    definitionOrder.append(match.id)
                } else {
                    // Duplicate id: keep the first.
                    currentDefinitionId = nil
                }
                continue
            }

            if let id = currentDefinitionId {
                // Continuation lines are either indented (at least 2 spaces
                // or a tab) or empty (ends the block on the NEXT non-indented
                // line).
                if line.isEmpty {
                    // Blank line: keep collecting — a blank inside a footnote
                    // is a paragraph break. Close only when the *next* line
                    // is non-indented.
                    definitions[id]?.lines.append("")
                    continue
                }
                if isIndentedContinuation(line) {
                    definitions[id]?.lines.append(trimmedContinuation(line))
                    continue
                }
                // Non-indented, non-empty line closes the definition.
                currentDefinitionId = nil
                cleanedLines.append(line)
                continue
            }

            cleanedLines.append(line)
        }

        // Trim trailing empty lines within each footnote's accumulated body.
        let orderedDefs: [Definition] = definitionOrder.enumerated().map { idx, id in
            var buf = definitions[id]?.lines ?? []
            while let last = buf.last, last.isEmpty { buf.removeLast() }
            return Definition(
                id: id,
                number: idx + 1,
                markdown: buf.joined(separator: "\n")
            )
        }

        return Output(
            cleanedMarkdown: cleanedLines.joined(separator: "\n"),
            definitions: orderedDefs
        )
    }

    // MARK: - Parsing helpers

    private struct DefinitionHeader {
        let id: String
        /// Markdown that follows `[^id]:` on the header line (first paragraph).
        let firstLine: String
    }

    /// Parse `[^id]: first-line-text`. Returns `nil` if the line doesn't match.
    private static func parseDefinitionHeader(_ line: String) -> DefinitionHeader? {
        // Must start with `[^` at column 0 (leading whitespace disqualifies —
        // indented `[^id]:` would be a continuation of an outer list item).
        guard line.hasPrefix("[^") else { return nil }
        let afterOpen = line.dropFirst(2)
        // Find closing bracket + ":" immediately after.
        guard let closeRange = afterOpen.range(of: "]:") else { return nil }
        let id = String(afterOpen[..<closeRange.lowerBound])
        guard !id.isEmpty, !id.contains(where: \.isWhitespace) else { return nil }
        var rest = String(afterOpen[closeRange.upperBound...])
        if rest.first == " " { rest.removeFirst() }
        return DefinitionHeader(id: id, firstLine: rest)
    }

    private static func isIndentedContinuation(_ line: String) -> Bool {
        if line.hasPrefix("\t") { return true }
        var spaces = 0
        for c in line {
            if c == " " {
                spaces += 1
                if spaces >= 2 { return true }
            } else {
                return false
            }
        }
        return false
    }

    private static func trimmedContinuation(_ line: String) -> String {
        if line.hasPrefix("\t") { return String(line.dropFirst()) }
        if line.hasPrefix("    ") { return String(line.dropFirst(4)) }
        if line.hasPrefix("  ") { return String(line.dropFirst(2)) }
        return line
    }
}
