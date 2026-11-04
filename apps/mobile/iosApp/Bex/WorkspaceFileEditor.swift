import AgentCore
import QuickLook
import SwiftUI

private struct WorkspaceFilePreview: View {
    let entry: FileEntry
    let download: @MainActor (String) async throws -> URL
    @Environment(\.dismiss) private var dismiss
    @State private var url: URL?
    @State private var error: String?
    @State private var attempt = UUID()

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading) {
                Text(entry.path).font(.caption).foregroundStyle(.secondary)
                    .textSelection(.enabled).padding(.horizontal)
                if let url {
                    if QLPreviewController.canPreview(url as NSURL) {
                        ConversationFilePreview(url: url)
                            .accessibilityIdentifier("file.preview")
                    } else {
                        AppNotice(text: "このファイル形式はプレビューできません").padding()
                    }
                } else if let error {
                    AppNotice(text: error).padding()
                    Button("再試行") { attempt = UUID() }.padding()
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .navigationTitle(entry.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("閉じる") { dismiss() }.accessibilityIdentifier("file.close")
                }
                ToolbarItem(placement: .confirmationAction) {
                    if let url {
                        ShareLink(item: url) { Image(systemName: "square.and.arrow.up") }
                            .accessibilityLabel("保存・共有")
                    }
                }
            }
        }
        .task(id: attempt) {
            error = nil
            do {
                let downloaded = try await download(entry.path)
                if Task.isCancelled {
                    try? FileManager.default.removeItem(at: downloaded.deletingLastPathComponent())
                    return
                }
                url = downloaded
            } catch {
                if !Task.isCancelled {
                    self.error = error.localizedDescription
                }
            }
        }
        .onDisappear {
            if let url {
                self.url = nil
                try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
            }
        }
    }
}

struct WorkspaceFileSheet: View {
    let snapshot: AgentCore.Snapshot
    @Binding var text: String
    let perform: SnapshotRequest
    let entry: FileEntry
    let line: UInt32?
    let download: @MainActor (String) async throws -> URL
    let aiEdit: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var error: String?
    @State private var busy = false
    @State private var initialized = false
    @State private var confirmReload = false
    @State private var selection: TextSelection?
    @State private var sharedFile: SharedFile?
    private var file: FileContent? {
        snapshot.file().flatMap { $0.path == entry.path ? $0 : nil }
    }

    private var revision: String {
        snapshot.fileDraft(path: entry.path)?.revision ?? file?.revision ?? ""
    }

    private var savedText: String {
        file?.text ?? ""
    }

    var body: some View {
        if mobileFilePreviewMimeType(path: entry.path) != nil {
            WorkspaceFilePreview(entry: entry, download: download)
        } else {
            editor
        }
    }

    private var editor: some View {
        NavigationStack {
            VStack(alignment: .leading) {
                Text(entry.path).font(.caption).foregroundColor(.secondary).textSelection(.enabled).padding(.horizontal)
                if let error {
                    AppNotice(text: error).padding(.horizontal)
                }
                if busy {
                    ProgressView().padding()
                }
                BufferedTextInput(value: $text) { TextEditor(text: $0, selection: $selection) }.font(.body.monospaced())
                    .textInputAutocapitalization(.never).disableAutocorrection(true)
                    .accessibilityIdentifier("file.editor")
                    .disabled(revision.isEmpty)
                HStack {
                    Button("再読込") {
                        if text != savedText {
                            confirmReload = true
                        } else {
                            load(restoreDraft: false)
                        }
                    }
                    Button("ダウンロード") {
                        busy = true; error = nil
                        Task {
                            defer { busy = false }
                            do {
                                sharedFile = try await SharedFile(url: download(entry.path))
                            } catch {
                                self.error = error.localizedDescription
                            }
                        }
                    }.disabled(busy)
                    Button("AIで編集") { aiEdit(entry.path) }.accessibilityIdentifier("file.ai-edit")
                }.padding()
            }
            .navigationTitle(entry.name)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("閉じる") { dismiss() }.accessibilityIdentifier("file.close")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        busy = true; error = nil
                        perform(.saveFile(SaveFile(path: entry.path))) { _, result in
                            busy = false
                            if case let .failure(failure) = result {
                                error = failure.localizedDescription
                            }
                        }
                    }.disabled(busy || revision.isEmpty || text == savedText).accessibilityIdentifier("file.save")
                }
            }
            .onChange(of: file?.revision) { moveToLine() }
            .sheet(item: $sharedFile) { item in FileShareSheet(url: item.url) }
            .onAppear {
                if !initialized {
                    load(restoreDraft: true)
                }
            }
            .confirmationDialog("保存していない編集を破棄して再読込しますか？", isPresented: $confirmReload, titleVisibility: .visible) {
                Button("編集を破棄して再読込", role: .destructive) {
                    load(restoreDraft: false)
                }
            }
        }
    }

    private func moveToLine() {
        guard let line else { return }
        let starts = [text.startIndex] + text.indices.filter { text[$0] == "\n" }.map { text.index(after: $0) }
        let start = starts[min(max(0, Int(line) - 1), starts.count - 1)]
        selection = TextSelection(range: start ..< start)
    }

    private func load(restoreDraft: Bool) {
        busy = true; error = nil
        perform(.readFile(ReadFile(path: entry.path, discardDraft: !restoreDraft))) { snapshot, result in
            busy = false
            if case let .failure(failure) = result {
                error = failure.localizedDescription; return
            }
            initialized = true
            if let file = snapshot.file(), file.path == entry.path,
               (snapshot.fileDraft(path: entry.path)?.revision ?? file.revision) != file.revision {
                error = "ホストのファイルが変更されています。下書きは保持しました。再読込すると下書きを破棄します。"
            }
        }
    }
}
