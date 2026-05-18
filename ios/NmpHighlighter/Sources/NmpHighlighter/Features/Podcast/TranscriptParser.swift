import Foundation

/// A single cue in a podcast transcript. Times are seconds from stream start.
struct TranscriptSegment: Hashable, Identifiable, Sendable {
    let id: String
    let start: TimeInterval
    let end: TimeInterval
    let speaker: String
    let text: String
}

/// Best-effort VTT / SRT / JSON transcript parser. Mirrors the dispatch +
/// shape-detection logic in `web/src/lib/server/podcasts.ts`. Returns `[]`
/// on failure — transcripts are informational and must never break the UI.
enum TranscriptParser {
    static func parse(data: Data, contentType: String?, fileExtension: String?) -> [TranscriptSegment] {
        guard let source = String(data: data, encoding: .utf8) else { return [] }
        let format = detectFormat(source: source, contentType: contentType, fileExtension: fileExtension)

        switch format {
        case .json: return parseJson(source: source)
        case .vtt:  return parseVtt(source: source)
        case .srt:  return parseSrt(source: source)
        case .unknown: return []
        }
    }

    // MARK: - Dispatch

    private enum Format { case vtt, srt, json, unknown }

    private static func detectFormat(source: String, contentType: String?, fileExtension: String?) -> Format {
        let ct = (contentType ?? "").lowercased()
        let ext = (fileExtension ?? "").lowercased()

        if ct.contains("json") || ext == "json" { return .json }
        if ct.contains("vtt") || ext == "vtt" { return .vtt }
        if ct.contains("srt") || ext == "srt" { return .srt }

        // Sniff the first 200 chars.
        let sniff = String(source.prefix(200)).trimmingCharacters(in: .whitespacesAndNewlines)
        if sniff.hasPrefix("WEBVTT") { return .vtt }
        if sniff.hasPrefix("[") || sniff.hasPrefix("{") { return .json }
        if sniff.contains("-->") { return .srt }
        return .unknown
    }

    // MARK: - VTT

    private static func parseVtt(source: String) -> [TranscriptSegment] {
        let normalized = source.replacingOccurrences(of: "\r", with: "")
        // Strip WEBVTT header (the first block until blank line)
        var body = normalized
        if body.hasPrefix("WEBVTT") {
            if let blank = body.range(of: "\n\n") {
                body = String(body[blank.upperBound...])
            } else {
                body = ""
            }
        }

        var segments: [TranscriptSegment] = []
        let blocks = body.components(separatedBy: "\n\n")

        for block in blocks {
            let lines = block
                .split(separator: "\n", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
            guard lines.count >= 2 else { continue }
            guard let timeIdx = lines.firstIndex(where: { $0.contains("-->") }) else { continue }

            let parts = splitTimecode(lines[timeIdx])
            guard let (start, end) = parts else { continue }

            let rawText = lines[(timeIdx + 1)...].joined(separator: "\n")
            let cleaned = stripVttTags(rawText)
            guard !cleaned.isEmpty else { continue }

            let speaker = extractVtt(lines[timeIdx + 1..<lines.count].joined(separator: " "))
                ?? extractSpeaker(cleaned) ?? ""
            let text = stripSpeakerPrefix(cleaned)

            segments.append(TranscriptSegment(
                id: "vtt-\(segments.count)",
                start: start,
                end: end,
                speaker: speaker,
                text: text
            ))
        }

        return segments
    }

    /// Extract speaker from `<v Speaker>…</v>` voice tag if present.
    private static func extractVtt(_ raw: String) -> String? {
        guard let range = raw.range(of: #"<v\s+([^>]+)>"#, options: .regularExpression) else { return nil }
        let match = String(raw[range])
        // Grab the capture manually.
        let trimmed = match
            .replacingOccurrences(of: "<v", with: "", options: .caseInsensitive)
            .trimmingCharacters(in: CharacterSet(charactersIn: " >"))
        return trimmed.isEmpty ? nil : trimmed
    }

    /// Remove `<v …>`, `<c …>` etc. style cue tags leaving the text content.
    private static func stripVttTags(_ raw: String) -> String {
        var result = raw
        // <v Speaker>Text</v> → Text (keep content, drop wrapping tag)
        if let regex = try? NSRegularExpression(pattern: #"<v[^>]*>([\s\S]*?)</v>"#, options: .caseInsensitive) {
            let ns = result as NSString
            result = regex.stringByReplacingMatches(
                in: result,
                range: NSRange(location: 0, length: ns.length),
                withTemplate: "$1"
            )
        }
        // Any remaining tags → strip
        result = result.replacingOccurrences(of: #"<[^>]+>"#, with: "", options: .regularExpression)
        return normalizeWhitespace(result)
    }

    // MARK: - SRT

    private static func parseSrt(source: String) -> [TranscriptSegment] {
        let normalized = source.replacingOccurrences(of: "\r", with: "")
        var segments: [TranscriptSegment] = []
        let blocks = normalized.components(separatedBy: "\n\n")

        for block in blocks {
            let lines = block
                .split(separator: "\n", omittingEmptySubsequences: false)
                .map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
            guard lines.count >= 2 else { continue }
            guard let timeIdx = lines.firstIndex(where: { $0.contains("-->") }) else { continue }

            guard let (start, end) = splitTimecode(lines[timeIdx]) else { continue }

            // Sequence number (prefer from the first line if it's an integer).
            let seq: String = {
                if let first = lines.first, Int(first) != nil { return first }
                return String(segments.count)
            }()

            let rawText = lines[(timeIdx + 1)...].joined(separator: "\n")
            let cleaned = normalizeWhitespace(rawText)
            guard !cleaned.isEmpty else { continue }
            let speaker = extractSpeaker(cleaned) ?? ""
            let text = stripSpeakerPrefix(cleaned)

            segments.append(TranscriptSegment(
                id: "srt-\(seq)",
                start: start,
                end: end,
                speaker: speaker,
                text: text
            ))
        }

        return segments
    }

    // MARK: - JSON

    private static func parseJson(source: String) -> [TranscriptSegment] {
        guard let data = source.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data, options: [])
        else { return [] }
        return findJsonSegments(json)
    }

    private static func findJsonSegments(_ value: Any) -> [TranscriptSegment] {
        if let array = value as? [Any] {
            // Try to interpret each entry as a segment directly.
            let direct = array.enumerated().compactMap { (idx, item) -> TranscriptSegment? in
                jsonSegment(item, index: idx)
            }
            if !direct.isEmpty { return direct }

            for item in array {
                let nested = findJsonSegments(item)
                if !nested.isEmpty { return nested }
            }
            return []
        }

        if let dict = value as? [String: Any] {
            for key in ["segments", "results", "items", "captions", "transcript"] {
                if let sub = dict[key] {
                    let nested = findJsonSegments(sub)
                    if !nested.isEmpty { return nested }
                }
            }
        }
        return []
    }

    private static func jsonSegment(_ value: Any, index: Int) -> TranscriptSegment? {
        guard let dict = value as? [String: Any] else { return nil }

        let text = firstString(dict, keys: ["text", "value", "caption", "body"])
        guard !text.isEmpty else { return nil }

        let start = firstDouble(dict, keys: ["start", "startTime", "start_time", "offset"]) ?? 0
        let end = firstDouble(dict, keys: ["end", "endTime", "end_time"]) ?? start

        let speaker = firstString(dict, keys: ["speaker", "speakerName", "speaker_name"])
        let id = firstString(dict, keys: ["id"])
        let normalizedText = normalizeWhitespace(text)

        return TranscriptSegment(
            id: id.isEmpty ? "json-\(index)" : id,
            start: start,
            end: end,
            speaker: speaker,
            text: normalizedText
        )
    }

    private static func firstString(_ dict: [String: Any], keys: [String]) -> String {
        for key in keys {
            if let s = dict[key] as? String, !s.isEmpty { return s }
        }
        return ""
    }

    private static func firstDouble(_ dict: [String: Any], keys: [String]) -> Double? {
        for key in keys {
            if let n = dict[key] as? Double { return n }
            if let n = dict[key] as? Int { return Double(n) }
            if let s = dict[key] as? String, let n = Double(s) { return n }
        }
        return nil
    }

    // MARK: - Shared helpers

    /// Splits `HH:MM:SS.mmm --> HH:MM:SS.mmm` (or comma variant) into two
    /// `TimeInterval`s. Returns `nil` if either side fails to parse.
    private static func splitTimecode(_ line: String) -> (TimeInterval, TimeInterval)? {
        let pieces = line.components(separatedBy: "-->")
        guard pieces.count == 2 else { return nil }
        guard let s = parseTimestamp(pieces[0].trimmingCharacters(in: .whitespaces)),
              let e = parseTimestamp(pieces[1].trimmingCharacters(in: .whitespaces))
        else { return nil }
        return (s, e)
    }

    /// Parses timestamps shaped like `HH:MM:SS.mmm`, `MM:SS.mmm`, or the
    /// comma-decimal SRT variants. Missing hours default to zero.
    static func parseTimestamp(_ raw: String) -> TimeInterval? {
        let pattern = #"(\d{1,2}):(\d{2})(?::(\d{2}))?(?:[.,](\d{1,3}))?"#
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: raw, range: NSRange(raw.startIndex..., in: raw))
        else { return nil }

        func group(_ i: Int) -> String? {
            guard i < match.numberOfRanges else { return nil }
            let r = match.range(at: i)
            guard r.location != NSNotFound, let range = Range(r, in: raw) else { return nil }
            return String(raw[range])
        }

        let first = Double(group(1) ?? "0") ?? 0
        let second = Double(group(2) ?? "0") ?? 0
        let third = (group(3)).flatMap { Double($0) }
        var ms: Double = 0
        if let msStr = group(4) {
            let padded = msStr.padding(toLength: 3, withPad: "0", startingAt: 0)
            ms = Double(padded) ?? 0
        }

        if let third {
            return first * 3600 + second * 60 + third + ms / 1000
        }
        return first * 60 + second + ms / 1000
    }

    /// Pulls a leading `Speaker:` prefix off a line and returns the name.
    private static func extractSpeaker(_ raw: String) -> String? {
        let pattern = #"^([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3}|[A-Z]{2,10})\s*:\s+"#
        guard let regex = try? NSRegularExpression(pattern: pattern),
              let match = regex.firstMatch(in: raw, range: NSRange(raw.startIndex..., in: raw)),
              match.numberOfRanges > 1,
              let range = Range(match.range(at: 1), in: raw)
        else { return nil }
        return String(raw[range]).trimmingCharacters(in: .whitespaces)
    }

    private static func stripSpeakerPrefix(_ raw: String) -> String {
        let pattern = #"^([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3}|[A-Z]{2,10})\s*:\s+"#
        return raw.replacingOccurrences(of: pattern, with: "", options: .regularExpression)
    }

    private static func normalizeWhitespace(_ raw: String) -> String {
        raw
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
