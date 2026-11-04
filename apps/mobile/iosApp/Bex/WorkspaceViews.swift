import AgentCore
import SwiftUI
import UIKit

extension FileEntry: @retroactive Identifiable {
    public var id: String {
        path
    }
}

extension ThreadScreen {
    func conversationDirectorySheet(_ destination: WorkspaceDestination) -> some View {
        NavigationStack {
            WorkspaceScreen(
                model: model,
                root: destination.directory,
                draft: destination.scope.draft,
                initialFile: destination.filePath.map { FileEntry(
                    name: ($0 as NSString).lastPathComponent,
                    path: $0,
                    directory: false,
                    size: 0
                ) },
                initialLine: destination.fileLine
            ) {
                openedDirectory = nil
            }
            .onDisappear {
                guard model.selectedProfileId == destination.scope.host,
                      model.coreDraftKey == destination.scope.draft else { return }
                model.perform(.reviewWorkspace(ReviewWorkspace(cwd: model.cwd)))
            }
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("閉じる") { openedDirectory = nil }
                }
            }
        }
    }
}

struct WorkspaceScreen: View {
    @ObservedObject var model: AppViewModel
    let root: String
    let draft: DraftKey
    var initialFile: FileEntry?
    var initialLine: UInt32?
    let close: () -> Void

    var body: some View {
        WorkspaceDirectoryScreen(
            snapshot: model.snapshot,
            directory: root,
            initialFile: initialFile,
            initialLine: initialLine,
            perform: request,
            fileDraft: fileDraft,
            downloadFile: model.download,
            aiEdit: { model.perform(.setDraftText(
                threadId: draft,
                text: "このファイルを編集してください: \($0)\n変更内容: "
            )) },
            close: close
        )
        .background(Color(uiColor: .systemGroupedBackground))
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
    let initialFile: FileEntry?
    let initialLine: UInt32?
    let perform: SnapshotRequest
    let fileDraft: (String) -> Binding<String>
    let downloadFile: @MainActor (String) async throws -> URL
    let aiEdit: (String) -> Void
    let close: () -> Void
    @State private var owner = UUID().uuidString
    @State private var refreshCompletion: CheckedContinuation<Void, Never>?
    @State private var selected: FileEntry?
    @State private var selectedLine: UInt32?
    @State private var sharedFile: SharedFile?
    @State private var error: ErrorText?
    @State private var busy = false

    private var root: String {
        fileHostPath(path: directory, home: snapshot.hostHomeDirectory())
    }

    private var key: FileTreeKey {
        FileTreeKey(owner: owner, root: root)
    }

    private var tree: FileTreeView {
        snapshot.fileTree(key: key)
    }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if let error = error ?? tree.error {
                        AppNotice(text: error).padding(.horizontal, 16).padding(.vertical, 8)
                    }
                    if tree.truncated {
                        Text("ほかにも一致する項目があります。検索語を絞り込んでください。")
                            .font(.caption).foregroundStyle(.secondary).padding(16)
                    }
                    if tree.rows.isEmpty {
                        emptyState
                    }
                    ForEach(tree.rows, id: \.path) { row in
                        Button { send(.click(path: row.path, additive: false, range: false)) } label: {
                            fileRow(row)
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(row.path)
                        .accessibilityIdentifier("file.\(row.label)")
                        .accessibilityAddTraits(row.selected ? .isSelected : [])
                        .id(row.path)
                    }
                }.padding(.vertical, 8)
            }
            .scrollDismissesKeyboard(.interactively)
            .refreshable {
                await withCheckedContinuation { continuation in
                    refreshCompletion?.resume()
                    refreshCompletion = continuation
                    perform(.fileTree(key: key, action: .refresh)) { next, _ in
                        if !next.fileTree(key: key).refreshing {
                            finishRefresh()
                        }
                    }
                }
            }
            .onChange(of: tree.refreshing) {
                if !tree.refreshing {
                    finishRefresh()
                }
            }
            .onChange(of: tree.revealId) {
                if let path = tree.focusedPath {
                    proxy.scrollTo(path)
                }
            }
        }
        .navigationTitle("ファイル")
        .navigationBarTitleDisplayMode(.inline)
        .searchable(text: Binding(get: { tree.query }, set: { send(.query(text: $0)) }), prompt: "ファイルを検索")
        .onAppear {
            send(.open(surface: .mobile, locale: Locale.current.identifier, workspaceRoot: root))
            if let initialFile {
                selectedLine = initialLine
                selected = initialFile
            }
        }
        .onDisappear { send(.close); finishRefresh() }
        .onChange(of: tree.openFile) { _, request in
            guard let request else { return }
            selectedLine = nil
            let path = fileTreePath(root: root, relative: request.path)
            selected = FileEntry(name: request.path.components(separatedBy: "/").last ?? request.path,
                                 path: path, directory: false, size: 0)
        }
        .sheet(item: $selected) { entry in
            WorkspaceFileSheet(snapshot: snapshot, text: fileDraft(entry.path), perform: perform,
                               entry: entry, line: selectedLine, download: downloadFile, share: download) { path in
                aiEdit(path)
                selected = nil
                close()
            }
        }
        .sheet(item: $sharedFile) { item in FileShareSheet(url: item.url) }
    }

    @ViewBuilder private var emptyState: some View {
        if tree.pending {
            ProgressView().frame(maxWidth: .infinity).padding(20)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                AppText(tree.error == nil ? "ファイルが見つかりません" : "ファイルを表示できません").fontWeight(.bold)
                if tree.error != nil {
                    Button("再試行") { send(.refresh) }
                } else {
                    AppText(tree.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        ? "作業スペースは空です。" : "別の語句で検索してください。").font(.caption)
                        .foregroundStyle(.secondary)
                }
            }.padding(16)
        }
    }

    private func fileRow(_ row: FileTreeRow) -> some View {
        HStack(spacing: 8) {
            Group {
                if row.kind == .directory {
                    Image(systemName: row.expanded ? "chevron.down" : "chevron.right")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                } else {
                    Color.clear
                }
            }.frame(width: 12)
            if row.kind == .directory {
                Image(systemName: "folder").font(.system(size: 17)).foregroundStyle(.secondary)
                    .frame(width: 17, height: 17)
            } else {
                Image(uiImage: mobileFileImage(row.path)).resizable().scaledToFit().frame(width: 17, height: 17)
            }
            Text(row.label).font(.system(size: 14, weight: row.selected ? .bold : .medium))
                .foregroundStyle(row.selected ? Color.primary : row
                    .ignored ? Color(uiColor: .tertiaryLabel) : .secondary)
                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            if row.kind == .directory, row.expanded, row.loading {
                ProgressView().controlSize(.small)
            }
        }
        .padding(.leading, 8 + CGFloat(row.depth) * 18).padding(.trailing, 8)
        .frame(minHeight: 42)
        .background(row.selected ? Color.primary.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 12))
        .contentShape(Rectangle()).padding(.horizontal, 8)
    }

    private func finishRefresh() {
        refreshCompletion?.resume()
        refreshCompletion = nil
    }

    private func send(_ action: FileTreeAction) {
        perform(.fileTree(key: key, action: action)) { _, _ in }
    }

    private func download(_ path: String) {
        guard !busy else { return }
        busy = true; error = nil
        Task {
            defer { busy = false }
            do {
                let url = try await downloadFile(path)
                selected = nil; sharedFile = SharedFile(url: url)
            } catch { self.error = errorText(error: error.localizedDescription) }
        }
    }
}

struct WorkspaceDiffScreen: View {
    @ObservedObject var model: AppViewModel
    let root: String
    @State private var files: [WorkspaceDiffFile]?

    var body: some View {
        // Core runs only the newest review read; an older request can finish
        // first, so the view follows the snapshot rather than its own request.
        let review = model.snapshot.review()
        Group {
            if let error = model.snapshot.operationError(key: .workspaceReview) {
                VStack {
                    AppNotice(text: error).padding()
                    Button("再試行", action: loadDiff).accessibilityIdentifier("files.diff.retry")
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let files, !files.isEmpty {
                ScrollView {
                    LazyVStack(spacing: 16) {
                        ForEach(files, id: \.path) { file in
                            WorkspaceDiffCard(file: file)
                        }
                    }.padding(12)
                }
            } else if files != nil {
                Text("変更はありません").foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear(perform: loadDiff)
        .task(id: ReviewIdentity(review: review)) {
            guard let review else { return }
            let next = await Task.detached(priority: .userInitiated) { review.diffFiles() }.value
            guard !Task.isCancelled else { return }
            files = next
        }
    }

    private func loadDiff() {
        files = nil
        model.perform(.reviewWorkspace(ReviewWorkspace(cwd: root)))
    }
}

private struct ReviewIdentity: Equatable {
    let review: WorkspaceReview?

    static func == (lhs: Self, rhs: Self) -> Bool {
        guard let left = lhs.review, let right = rhs.review else { return lhs.review == nil && rhs.review == nil }
        return left.unchanged(other: right)
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
                            AppText(agent.status).foregroundStyle(.secondary)
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
