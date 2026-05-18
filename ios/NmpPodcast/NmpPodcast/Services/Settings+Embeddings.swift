import Foundation

// MARK: - Embedding constants
//
// Display-only metadata for the on-device embedding pipeline. Surfaced in the AI
// settings UI so the user can confirm what the RAG layer is using.

extension Settings {
    static let defaultEmbeddingsModel: String = "openai/text-embedding-3-large"
    static let embeddingsModelID: String = defaultEmbeddingsModel
    static let embeddingsDimensions: Int = 1024

    static func embeddingsModelDisplay(modelID: String, modelName: String = "") -> String {
        "\(modelDisplayName(modelID: modelID, modelName: modelName))@\(embeddingsDimensions)"
    }

    static var embeddingsModelDisplay: String {
        embeddingsModelDisplay(modelID: defaultEmbeddingsModel)
    }
}
