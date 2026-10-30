import AgentCore
import SwiftUI
import UIKit

extension FileEntry: @retroactive Identifiable {
    public var id: String {
        path
    }
}

struct WorkspaceScreen: View {
    @ObservedObject var model: AppViewModel
    let root: String
    let draft: DraftKey
    let close: () -> Void

    var body: some View {
        WorkspaceDirectoryScreen(snapshot: model.snapshot, directory: root, title: "ファイル",
                                 perform: request, fileDraft: fileDraft,
                                 downloadFile: model.download,
                                 aiEdit: { model.perform(.setDraftText(
                                     threadId: draft,
                                     text: "このファイルを編集してください: \($0)\n変更内容: "
                                 )) }, close: close)
            .background(Color(uiColor: .systemGroupedBackground))
            .onDisappear { model.perform(.reviewWorkspace(ReviewWorkspace(cwd: root))) }
    }

    private func request(_ intent: Intent, completion: @escaping (AgentCore.Snapshot, Result<Outcome, Error>) -> Void) {
        model.requestSnapshot(intent) { snapshot, result in
            model.notice = nil
            completion(snapshot, result)
        }
    }

    private func fileDraft(_ path: String) -> Binding<String> {
        Binding(get: {
            model.snapshot.fileDraft(path: path)?.text ??
                model.snapshot.file().flatMap { $0.path == path ? $0.text : nil } ?? ""
        }, set: { model.perform(.setFileDraft(path: path, text: $0)) })
    }
}

private struct WorkspaceDirectoryScreen: View {
    let snapshot: AgentCore.Snapshot
    let directory: String
    let title: String
    let perform: SnapshotRequest
    let fileDraft: (String) -> Binding<String>
    let downloadFile: @MainActor (String) async throws -> URL
    let aiEdit: (String) -> Void
    let close: () -> Void
    @State private var openedDirectory: String?
    @State private var path = ""
    @State private var entries: [FileEntry] = []
    @State private var error: String?
    @State private var busy = false
    @State private var selected: FileEntry?
    @State private var sharedFile: SharedFile?

    var body: some View {
        List {
            if let error {
                AppNotice(text: error)
            }
            if busy {
                ProgressView()
            }
            Section {
                HStack {
                    TextField("絶対パス", text: $path)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .submitLabel(.go).onSubmit { openedDirectory = path }
                    Button("開く") { openedDirectory = path }
                        .accessibilityIdentifier("files.open-path")
                }
                Button { openedDirectory = (directory as NSString).deletingLastPathComponent } label: {
                    Label("親ディレクトリ", systemImage: "arrow.up")
                }
                .disabled(directory == "/")
            }
            Section {
                ForEach(entries) { entry in
                    HStack {
                        if entry.directory {
                            Button { openedDirectory = entry.path } label: {
                                HStack {
                                    Label(entry.name, systemImage: "folder")
                                    Spacer()
                                    Image(systemName: "chevron.right")
                                        .font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
                                }
                            }
                            .accessibilityIdentifier("file.\(entry.name)")
                        } else {
                            Button { selected = entry } label: { Label(entry.name, systemImage: "doc") }
                                .buttonStyle(.borderless)
                                .accessibilityIdentifier("file.\(entry.name)")
                        }
                        Spacer()
                        if !entry.directory {
                            Button { download(entry.path) } label: { Image(systemName: "square.and.arrow.down") }
                                .buttonStyle(.borderless).accessibilityLabel("\(entry.name)をダウンロード")
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(item: $openedDirectory) { directory in
            WorkspaceDirectoryScreen(snapshot: snapshot, directory: directory,
                                     title: URL(fileURLWithPath: directory).lastPathComponent,
                                     perform: perform, fileDraft: fileDraft, downloadFile: downloadFile,
                                     aiEdit: aiEdit, close: close)
        }
        .onAppear {
            if path.isEmpty {
                load(directory)
            }
        }
        .sheet(item: $selected) { entry in
            FileEditorSheet(
                snapshot: snapshot,
                text: fileDraft(entry.path),
                perform: perform,
                entry: entry,
                download: download
            ) { path in
                aiEdit(path)
                selected = nil
                close()
            }
        }
        .sheet(item: $sharedFile, onDismiss: cleanupDownload) { item in
            FileShareSheet(url: item.url)
        }
    }

    private func load(_ directory: String) {
        path = directory
        busy = true; error = nil
        perform(.listFiles(ListFiles(path: directory))) { snapshot, result in
            busy = false
            if case let .failure(failure) = result {
                error = failure.localizedDescription; return
            }
            guard let result = snapshot.directory(), result.path == directory else {
                error = snapshot.operationError(key: .directory)
                return
            }
            path = result.path
            entries = result.entries
            if result.truncated {
                error = "先頭 2,000 件を表示しています。パスを指定して開けます。"
            }
        }
    }

    private func download(_ path: String) {
        busy = true; error = nil
        Task {
            defer { busy = false }
            do {
                let url = try await downloadFile(path)
                selected = nil; sharedFile = SharedFile(url: url)
            } catch { self.error = error.localizedDescription }
        }
    }

    private func cleanupDownload() {
        if let directory = sharedFile?.url
            .deletingLastPathComponent() {
            try? FileManager.default.removeItem(at: directory)
        }
        sharedFile = nil
    }
}

struct WorkspaceDiffScreen: View {
    @ObservedObject var model: AppViewModel
    let root: String
    @State private var files: [WorkspaceDiffFile] = []
    @State private var error: String?
    @State private var busy = true

    var body: some View {
        Group {
            if busy {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error {
                VStack {
                    AppNotice(text: error).padding()
                    Button("再試行", action: loadDiff).accessibilityIdentifier("files.diff.retry")
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if files.isEmpty {
                Text("変更はありません").foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 16) {
                        ForEach(files, id: \.path) { file in
                            WorkspaceDiffCard(file: file)
                        }
                    }.padding(12)
                }
            }
        }
        .onAppear(perform: loadDiff)
    }

    private func loadDiff() {
        busy = true; error = nil
        model.requestSnapshot(.reviewWorkspace(ReviewWorkspace(cwd: root))) { snapshot, result in
            model.notice = nil
            if case let .failure(failure) = result {
                busy = false
                error = failure.localizedDescription
            } else if let review = snapshot.review() {
                Task {
                    files = await Task.detached(priority: .userInitiated) { review.diffFiles() }.value
                    busy = false
                }
            } else {
                busy = false
                error = "差分を取得できませんでした。再試行してください。"
            }
        }
    }
}

struct WorkspaceDiffCard: View {
    let file: WorkspaceDiffFile
    @State private var expanded = true

    var body: some View {
        LazyVStack(spacing: 0) {
            Button { expanded.toggle() } label: {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: expanded ? "chevron.down" : "chevron.right")
                    Text(file.path).fontWeight(.semibold).frame(maxWidth: .infinity, alignment: .leading)
                    if let additions = file.additions {
                        Text("+\(additions)").foregroundColor(.green)
                    }
                    if let deletions = file.deletions {
                        Text("−\(deletions)").foregroundColor(.red)
                    }
                }.font(.subheadline).padding(12).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("diff.file.\(file.path)")
            .background(Color(uiColor: .secondarySystemGroupedBackground))
            if expanded {
                if file.rows.isEmpty {
                    Text("テキスト差分はありません").font(.caption).foregroundColor(.secondary).padding()
                }
                ForEach(file.rows.indices, id: \.self) { index in
                    let row = file.rows[index]
                    HStack(alignment: .top, spacing: 0) {
                        if row.kind == "+" || row.kind == "-" || row.kind == " " {
                            Text((row.new ?? row.old).map(String.init) ?? "")
                                .foregroundColor(.secondary).frame(width: 40, alignment: .trailing).padding(
                                    .trailing,
                                    8
                                )
                        }
                        Text(row.text.isEmpty ? " " : row.text)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .fixedSize(horizontal: false, vertical: true)
                            .textSelection(.enabled)
                    }
                    .font(.system(.footnote, design: .monospaced))
                    .padding(.vertical, 3).padding(.horizontal, 8)
                    .background(row.kind == "+" ? Color.green.opacity(0.18) :
                        row.kind == "-" ? Color.red.opacity(0.18) :
                        row.kind == "@" ? Color.secondary.opacity(0.12) : Color.clear)
                }
            }
        }
        .background(Color(uiColor: .systemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
    }
}

struct WorkspaceAgentsScreen: View {
    let agents: [AgentSummary]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if agents.isEmpty {
                    Text("サブエージェントが起動するとここに表示されます。")
                        .foregroundStyle(.secondary)
                }
                ForEach(agents, id: \.key) { agent in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            if agent.active {
                                ProgressView().controlSize(.small)
                            }
                            Text(agent.title)
                            Spacer()
                            Text(agent.status).foregroundStyle(.secondary)
                        }
                        if let detail = agent.detail {
                            Text(detail).foregroundStyle(.secondary)
                        }
                        if let model = agent.model {
                            Text(model).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .padding(12).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                    .padding(.leading, CGFloat(agent.depth) * 16)
                }
            }.font(.subheadline).padding(16).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
