import Observation
import QuickLook
import SwiftUI

/// User-initiated presentations outlive recycled rows. Inline loads remain owned by each image or web view.
@MainActor @Observable final class ConversationMediaPresentation {
    struct Visualization: Identifiable {
        let id = UUID()
        let html: String
    }

    struct FileFailure {
        let path: String
        let message: String
    }

    var image: SharedFile?
    var sharedFile: SharedFile?
    var visualization: Visualization?
    var imageError: String?
    private(set) var shareFailure: FileFailure?
    private(set) var linkFailure: FileFailure?
    @ObservationIgnored private var pending: Task<Void, Never>?

    isolated deinit {
        pending?.cancel()
        for url in [image?.url, sharedFile?.url].compactMap(\.self) {
            try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
        }
    }

    func openImage(_ data: Data) {
        imageError = nil
        load { try await writeConversationImage(data) } present: {
            self.image = SharedFile(url: $0)
        } failed: { self.imageError = $0 }
    }

    func share(_ path: String, download: @escaping @MainActor (String) async throws -> URL) {
        shareFailure = nil
        load { try await download(path) } present: {
            self.sharedFile = SharedFile(url: $0)
        } failed: { self.shareFailure = FileFailure(path: path, message: $0) }
    }

    func openLink(_ target: String,
                  resolve: @escaping @MainActor (String) async throws -> ConversationMediaAccess.ResolvedFile,
                  openDirectory: @escaping @MainActor (String) -> Void,
                  openFile: @escaping @MainActor (String, UInt32?) -> Void) {
        linkFailure = nil
        perform {
            let entry = try await resolve(target)
            try Task.checkCancellation()
            if entry.isDirectory {
                openDirectory(entry.path)
            } else {
                openFile(entry.path, entry.line)
            }
        } failed: { self.linkFailure = FileFailure(path: target, message: $0) }
    }

    private func load(_ operation: @escaping @MainActor () async throws -> URL,
                      present: @escaping @MainActor (URL) -> Void,
                      failed: @escaping @MainActor (String) -> Void) {
        perform({
            let url = try await operation()
            guard !Task.isCancelled else {
                try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                return
            }
            present(url)
        }, failed: failed)
    }

    private func perform(_ operation: @escaping @MainActor () async throws -> Void,
                         failed: @escaping @MainActor (String) -> Void) {
        cancelPending()
        pending = Task {
            defer {
                if !Task.isCancelled {
                    pending = nil
                }
            }
            do {
                try await operation()
            } catch {
                if !Task.isCancelled {
                    failed(error.localizedDescription)
                }
            }
        }
    }

    func closeImage() {
        remove(image)
        image = nil
    }

    /// The share sheet removes the file itself when sharing ends; a reset may end it earlier.
    private func closeShare() {
        remove(sharedFile)
        sharedFile = nil
    }

    func cancelPending() {
        pending?.cancel()
        pending = nil
    }

    func reset() {
        cancelPending()
        closeImage()
        closeShare()
        visualization = nil
        imageError = nil
        shareFailure = nil
        linkFailure = nil
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
            // QuickLook owns zooming, sharing and saving to Photos.
            .quickLookPreview(Binding(get: { presentation.image?.url }, set: {
                if $0 == nil {
                    presentation.closeImage()
                }
            }))
            .shareSheet($presentation.sharedFile)
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
            .alert("画像を開けません", isPresented: Binding(get: { presentation.imageError != nil }, set: {
                if !$0 {
                    presentation.imageError = nil
                }
            })) {
                Button("OK") { presentation.imageError = nil }
            } message: { AppText(presentation.imageError ?? "") }
    }
}
