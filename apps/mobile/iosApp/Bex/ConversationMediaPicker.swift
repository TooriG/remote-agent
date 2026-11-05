import AgentCore
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// One stable presentation owner serves both positions of the attachment menu.
struct ComposerMediaPicker: View {
    enum Kind { case file, photos, camera }
    let model: ConversationModel
    @Binding var selection: Kind?
    @Binding var preparingMedia: Bool
    @State private var selectedPhotos: [PhotosPickerItem] = []

    var body: some View {
        Color.clear.frame(width: 0, height: 0)
            .fileImporter(isPresented: presenting(.file), allowedContentTypes: [.item]) { result in
                Task {
                    do { try await model.attach(result.get()) } catch {
                        model.transferError = error.localizedDescription
                    }
                }
            }
            .photosPicker(isPresented: presenting(.photos), selection: $selectedPhotos,
                          selectionBehavior: .ordered, matching: .any(of: [.images, .videos]),
                          preferredItemEncoding: .compatible)
            .onChange(of: selectedPhotos) { _, items in
                guard !items.isEmpty else { return }
                selectedPhotos = []
                preparingMedia = true
                model.transferError = nil
                let draftKey = model.draftKey
                Task { await importPhotos(items, draftKey: draftKey) }
            }
            .fullScreenCover(isPresented: presenting(.camera)) {
                ChatCameraPicker { result in
                    selection = nil
                    Task {
                        do {
                            if let url = try result.get() {
                                try await model.attach(url, temporaryDirectory: url.deletingLastPathComponent())
                            }
                        } catch { model.transferError = error.localizedDescription }
                    }
                }.ignoresSafeArea()
            }
    }

    private func presenting(_ kind: Kind) -> Binding<Bool> {
        Binding(get: { selection == kind }, set: {
            if !$0, selection == kind {
                selection = nil
            }
        })
    }
}

struct ComposerAttachMenu: View {
    let disabled: Bool
    let choose: (ComposerMediaPicker.Kind) -> Void
    let camera: () -> Void

    var body: some View {
        Menu {
            Button { choose(.photos) } label: { Label("写真・動画", systemImage: "photo.on.rectangle") }
                .accessibilityIdentifier("task.attach.photos")
            Button(action: camera) { Label("カメラ", systemImage: "camera") }
                .accessibilityIdentifier("task.attach.camera")
            Button { choose(.file) } label: { Label("ファイル", systemImage: "doc") }
                .accessibilityIdentifier("task.attach.file")
        } label: {
            Image(systemName: "plus").font(.title2.weight(.regular)).frame(width: 40, height: 40)
        }
        .disabled(disabled)
        .accessibilityLabel("添付").accessibilityIdentifier("task.attach")
    }
}

struct DictationWaveform: View {
    let levels: [Float]

    var body: some View {
        Canvas { context, size in
            let spacing = size.width / CGFloat(levels.count)
            for (index, level) in levels.enumerated() {
                let height = 3 + CGFloat(level.squareRoot()) * (size.height - 3)
                let bar = CGRect(x: CGFloat(index) * spacing, y: (size.height - height) / 2,
                                 width: 3, height: height)
                context.fill(Path(roundedRect: bar, cornerRadius: 1.5), with: .foreground)
            }
        }
        .frame(height: 48)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("録音中")
        .accessibilityIdentifier("dictation.recording")
    }
}
