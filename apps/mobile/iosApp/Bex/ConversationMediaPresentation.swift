import Observation
import SwiftUI

/// User-initiated presentations outlive recycled rows. Inline loads remain owned by each image or web view.
@MainActor @Observable final class ConversationMediaPresentation {
    struct Visualization: Identifiable {
        let id = UUID()
        let html: String
    }

    var image: SharedFile?
    var sharedFile: SharedFile?
    var visualization: Visualization?
    var error: String?
    @ObservationIgnored private var pending: Task<Void, Never>?

    deinit {
        pending?.cancel()
        for url in [image?.url, sharedFile?.url].compactMap(\.self) {
            try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
        }
    }

    func openImage(_ data: Data) {
        load { try await writeConversationImage(data) } present: { self.image = SharedFile(url: $0) }
    }

    func share(_ path: String, download: @escaping @MainActor (String) async throws -> URL) {
        load { try await download(path) } present: { self.sharedFile = SharedFile(url: $0) }
    }

    private func load(_ operation: @escaping @MainActor () async throws -> URL,
                      present: @escaping @MainActor (URL) -> Void) {
        guard pending == nil else { return }
        error = nil
        pending = Task {
            defer {
                if !Task.isCancelled {
                    pending = nil
                }
            }
            do {
                let url = try await operation()
                guard !Task.isCancelled else {
                    try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                    return
                }
                present(url)
            } catch {
                if !Task.isCancelled {
                    self.error = error.localizedDescription
                }
            }
        }
    }

    func closeImage() {
        remove(image)
        image = nil
    }

    func closeShare() {
        remove(sharedFile)
        sharedFile = nil
    }

    func reset() {
        pending?.cancel()
        pending = nil
        closeImage()
        closeShare()
        visualization = nil
        error = nil
    }

    private func remove(_ file: SharedFile?) {
        if let file {
            try? FileManager.default.removeItem(at: file.url.deletingLastPathComponent())
        }
    }
}

struct ConversationMediaPresenter: ViewModifier {
    @Bindable var presentation: ConversationMediaPresentation

    func body(content: Content) -> some View {
        content
            .fullScreenCover(item: $presentation.image, onDismiss: presentation.closeImage) { image in
                ConversationPreview(url: image.url, isImage: true, close: presentation.closeImage)
            }
            .sheet(item: $presentation.sharedFile, onDismiss: presentation.closeShare) { file in
                FileShareSheet(url: file.url)
            }
            .sheet(item: $presentation.visualization) { document in
                NavigationStack {
                    VisualizationWebView(html: document.html)
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) {
                                Button("閉じる") { presentation.visualization = nil }
                            }
                        }
                }
            }
            .alert("エラー", isPresented: Binding(get: { presentation.error != nil }, set: {
                if !$0 {
                    presentation.error = nil
                }
            })) {
                Button("OK") { presentation.error = nil }
            } message: { AppText(presentation.error ?? "") }
    }
}
