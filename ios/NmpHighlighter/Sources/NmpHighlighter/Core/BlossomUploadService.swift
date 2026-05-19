import CryptoKit
import Foundation

/// Reusable actor for uploading blobs to the user's Blossom servers (BUD-01).
/// Tries each server in the user's kind:10063 list in order; returns the URL
/// from the first server that accepts the upload.
///
/// Auth uses NIP-98 HTTP Auth (kind:27235) signed by the Rust core.
actor BlossomUploadService {
    private let safeCore: SafeHighlighterCore

    init(safeCore: SafeHighlighterCore) {
        self.safeCore = safeCore
    }

    /// Upload `data` to the first available Blossom server.
    /// Returns the canonical URL of the stored blob.
    func upload(data: Data, mimeType: String) async throws -> URL {
        let servers = try await safeCore.getBlossomServers()
        guard !servers.isEmpty else {
            throw BlossomUploadError.noServersConfigured
        }

        let hash = SHA256.hash(data: data).hexString
        var lastError: Error = BlossomUploadError.noServersConfigured

        for serverBase in servers {
            let base = serverBase.trimmingSuffix("/")
            let uploadURLString = "\(base)/upload"
            guard let endpoint = URL(string: uploadURLString) else { continue }

            do {
                let authJSON = try await safeCore.signNip98Auth(
                    url: uploadURLString,
                    method: "PUT",
                    payloadHash: hash
                )
                let authBase64 = Data(authJSON.utf8).base64EncodedString()

                var request = URLRequest(url: endpoint)
                request.httpMethod = "PUT"
                request.setValue("Nostr \(authBase64)", forHTTPHeaderField: "Authorization")
                request.setValue(mimeType, forHTTPHeaderField: "Content-Type")
                request.setValue("\(data.count)", forHTTPHeaderField: "Content-Length")

                let (responseData, response) = try await URLSession.shared.upload(
                    for: request,
                    from: data
                )
                guard let http = response as? HTTPURLResponse,
                      (200...299).contains(http.statusCode) else {
                    let status = (response as? HTTPURLResponse)?.statusCode ?? 0
                    throw BlossomUploadError.serverError(status)
                }

                // BUD-01 servers return a JSON object with a `url` field.
                if let json = try? JSONSerialization.jsonObject(with: responseData) as? [String: Any],
                   let urlString = json["url"] as? String,
                   let url = URL(string: urlString) {
                    return url
                }
                // Fallback: construct the canonical URL from the hash.
                return URL(string: "\(base)/\(hash)")!
            } catch {
                lastError = error
                continue
            }
        }
        throw lastError
    }
}

enum BlossomUploadError: Error, LocalizedError {
    case noServersConfigured
    case serverError(Int)

    var errorDescription: String? {
        switch self {
        case .noServersConfigured:
            return "No Blossom servers configured. Add one in Settings → Media."
        case .serverError(let code):
            return "Upload failed (HTTP \(code))."
        }
    }
}

private extension String {
    func trimmingSuffix(_ suffix: String) -> String {
        hasSuffix(suffix) ? String(dropLast(suffix.count)) : self
    }
}

private extension Digest {
    var hexString: String {
        makeIterator().map { String(format: "%02x", $0) }.joined()
    }
}
