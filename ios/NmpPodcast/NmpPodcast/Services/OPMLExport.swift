import Foundation

/// Produces an OPML 2.0 document from the user's current subscriptions.
///
/// Output shape mirrors what Apple Podcasts / Pocket Casts emit so re-importing
/// our export into another app is lossless. Hand-written generation (no third
/// party): SPM dependencies are forbidden in this lane.
struct OPMLExport: Sendable {

    /// Generates an OPML 2.0 document from the followed podcasts.
    ///
    /// - Parameters:
    ///   - podcasts: The list to export. Order is preserved.
    ///   - title: Title written into `<head><title>`. Defaults to *Podcastr Subscriptions*.
    ///   - dateCreated: Optional override for `<head><dateCreated>`. Defaults to now.
    /// - Returns: UTF-8 OPML bytes suitable for sharing or writing to disk.
    func exportOPML(
        podcasts: [Podcast],
        title: String = "Podcastr Subscriptions",
        dateCreated: Date = Date()
    ) -> Data {
        var lines: [String] = []
        lines.append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")
        lines.append("<opml version=\"2.0\">")
        lines.append("  <head>")
        lines.append("    <title>\(escape(title))</title>")
        lines.append("    <dateCreated>\(rfc822(dateCreated))</dateCreated>")
        lines.append("  </head>")
        lines.append("  <body>")
        lines.append("    <outline text=\"feeds\" title=\"feeds\">")

        for podcast in podcasts {
            if let line = makeOutline(for: podcast) {
                lines.append(line)
            }
        }

        lines.append("    </outline>")
        lines.append("  </body>")
        lines.append("</opml>")

        return lines.joined(separator: "\n").data(using: .utf8) ?? Data()
    }

    // MARK: Private

    /// Returns the outline XML row for a podcast, or `nil` for synthetic
    /// podcasts (no feed URL — nothing to round-trip into another app).
    private func makeOutline(for podcast: Podcast) -> String? {
        guard let feedURL = podcast.feedURL else { return nil }
        var attrs: [(String, String)] = [
            ("type", "rss"),
            ("text", podcast.title),
            ("title", podcast.title),
            ("xmlUrl", feedURL.absoluteString),
        ]
        if !podcast.description.isEmpty {
            attrs.append(("description", podcast.description))
        }
        if let language = podcast.language, !language.isEmpty {
            attrs.append(("language", language))
        }
        let rendered = attrs
            .map { "\($0.0)=\"\(escape($0.1))\"" }
            .joined(separator: " ")
        return "      <outline \(rendered) />"
    }

    /// Minimal XML attribute escaping. Covers the five XML predefined entities
    /// plus a CR/LF fold so attribute values stay on one line.
    private func escape(_ s: String) -> String {
        var out = ""
        out.reserveCapacity(s.count)
        for ch in s {
            switch ch {
            case "&": out.append("&amp;")
            case "<": out.append("&lt;")
            case ">": out.append("&gt;")
            case "\"": out.append("&quot;")
            case "'": out.append("&apos;")
            case "\n", "\r": out.append(" ")
            default: out.append(ch)
            }
        }
        return out
    }

    private func rfc822(_ date: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "GMT")
        f.dateFormat = "EEE, dd MMM yyyy HH:mm:ss zzz"
        return f.string(from: date)
    }
}
