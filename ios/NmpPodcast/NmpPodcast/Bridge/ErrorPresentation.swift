import SwiftUI
import OSLog

struct AppError: Identifiable {
    let id = UUID()
    let title: String
    let message: String
    let underlyingError: Error?

    init(title: String, message: String, underlyingError: Error? = nil) {
        self.title = title
        self.message = message
        self.underlyingError = underlyingError
    }

    init(title: String, error: Error) {
        self.title = title
        self.message = error.localizedDescription
        self.underlyingError = error
    }
}

extension View {
    func errorAlert(error: Binding<AppError?>) -> some View {
        alert(item: error) { appError in
            Alert(
                title: Text(appError.title),
                message: Text(appError.message),
                dismissButton: .default(Text("OK"))
            )
        }
    }
}

@MainActor
@Observable
final class ErrorHandler {
    static let shared = ErrorHandler()

    var currentError: AppError?

    private init() {}

    func handle(_ error: Error, title: String, file: String = #file, function: String = #function, line: Int = #line) {
        let fileName = (file as NSString).lastPathComponent
        Logger.general.error("[\(fileName):\(line)] \(function) - \(title): \(error.localizedDescription)")
        currentError = AppError(title: title, error: error)
    }

    func show(title: String, message: String) {
        Logger.general.warning("\(title): \(message)")
        currentError = AppError(title: title, message: message)
    }

    func clear() {
        currentError = nil
    }
}
