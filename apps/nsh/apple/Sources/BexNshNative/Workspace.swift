import SwiftUI

struct WorkspaceSummary: Decodable {
    struct Editor: Decodable, Identifiable { let id: String; let title: String; let selected: Bool }
    struct Source: Decodable { let path: String; let title: String }
    let directory: String
    let enabled: Bool
    let hasDirectory: Bool
    let branch: String
    let additions: UInt64
    let deletions: UInt64
    let editors: [Editor]
    let editor: String?
    let editorLabel: String
    let sources: [Source]
}

struct WorkspaceCard: View {
    @Environment(\.colorScheme) private var colorScheme
    let summary: WorkspaceSummary
    let send: ([String: String]) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("ワークスペース").foregroundStyle(.secondary)
                    Spacer()
                    Button { action("refresh") } label: { Image(systemName: "arrow.clockwise") }
                        .accessibilityLabel("変更を更新")
                        .disabled(!summary.hasDirectory)
                }
                Button { action("files") } label: { Label(summary.directory, systemImage: "folder") }
                    .accessibilityIdentifier("workspace.files")
                if !summary.editors.isEmpty {
                    HStack {
                        Button {
                            if let editor = summary.editor {
                                send(["action": "editor", "editor": editor])
                            }
                        } label: {
                            Label(summary.editorLabel, systemImage: "arrow.up.right.square")
                        }.disabled(summary.editor == nil || !summary.hasDirectory)
                        Spacer()
                        Menu {
                            ForEach(summary.editors) { editor in
                                Button { send(["action": "preferredEditor", "editor": editor.id]) } label: {
                                    if editor.selected {
                                        Label(editor.title, systemImage: "checkmark")
                                    } else {
                                        Text(verbatim: editor.title)
                                    }
                                }
                            }
                        } label: { Image(systemName: "chevron.down") }
                            .accessibilityLabel("エディターを選択")
                            .disabled(!summary.hasDirectory)
                        #if os(macOS)
                            .menuStyle(.borderlessButton)
                            .menuIndicator(.hidden)
                        #endif
                    }
                }
                if !summary.branch.isEmpty {
                    Text(verbatim: summary.branch).foregroundStyle(.secondary)
                }
                Button { action("diff") } label: {
                    HStack {
                        Label("変更", systemImage: "arrow.left.arrow.right")
                        Spacer()
                        Text("+\(summary.additions)").foregroundStyle(.green)
                        Text("−\(summary.deletions)").foregroundStyle(.red)
                    }
                }.accessibilityIdentifier("workspace.diff").disabled(!summary.hasDirectory)
                Divider().padding(.vertical, 4)
                HStack {
                    Text("ソース").foregroundStyle(.secondary)
                    Spacer()
                    Button { action("attach") } label: { Image(systemName: "plus") }
                        .accessibilityLabel("ソースを追加")
                }
                if summary.sources.isEmpty {
                    Text("添付ファイルがここに表示されます").foregroundStyle(.secondary).padding(.vertical, 4)
                }
                ForEach(summary.sources, id: \.path) { source in
                    Button { send(["action": "source", "path": source.path]) } label: { Label(
                        source.title,
                        systemImage: "doc"
                    ) }
                }
                Button { action("files") } label: { Label("すべてのファイル", systemImage: "folder") }
            }
            .font(.system(size: 13, weight: .regular)).buttonStyle(.plain)
            .disabled(!summary.enabled)
            .padding(16).frame(maxWidth: .infinity, alignment: .leading)
            .background(
                colorScheme == .dark ? Color(white: 20.0 / 255.0) : Color(white: 0.96),
                in: RoundedRectangle(cornerRadius: 18)
            )
        }
    }

    private func action(_ name: String) {
        send(["action": name])
    }
}
