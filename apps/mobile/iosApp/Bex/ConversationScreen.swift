import AgentCore
import SwiftUI
import UIKit

struct ConversationEnvironment {
    let profiles: [HostProfile]
    let hostName: String?
    let list: ThreadList?
    let connecting: Bool
    var selectHost: ((String) -> Void)?
}

struct ThreadScreen: View {
    @Environment(\.locale) private var locale
    let model: ConversationModel
    let environment: ConversationEnvironment
    let conversation: ConversationPresentation?
    var isSideChat = false
    var openTools: ((PanelKind?) -> Void)?
    @State var openedDirectory: WorkspaceDestination?
    @State var mediaPresentation = ConversationMediaPresentation()
    @State var scroll = ConversationScrollState()
    @State var expandedItemIds = Set<String>()
    @State var activityExpansionOverrides = [String: String]()

    var body: some View {
        chatContent
            .modifier(ConversationMediaPresenter(presentation: mediaPresentation))
            .onChange(of: model.draftKey) {
                expandedItemIds.removeAll()
                activityExpansionOverrides.removeAll()
                scroll.goTo(.bottom)
                openedDirectory = nil
                mediaPresentation.reset()
            }
            .sheet(item: $openedDirectory) { destination in
                ConversationDirectorySheet(model: model, destination: destination) { openedDirectory = nil }
            }
            .navigationTitle(isSideChat ? Text("サイドチャット") : conversationTitleText(
                model.conversationContext.title(), fallback: model.isNewThread ? "新しいタスク" : "タスク"
            ))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if !isSideChat {
                    if !model.isNewThread {
                        ToolbarItem(placement: .principal) {
                            Button {
                                scroll.goTo(.top)
                            } label: {
                                conversationTitle
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("会話の先頭へ")
                            .accessibilityIdentifier("task.top")
                        }
                    }
                    ToolbarItemGroup(placement: .navigationBarTrailing) {
                        Button {
                            UIApplication.shared.sendAction(
                                #selector(UIResponder.resignFirstResponder),
                                to: nil,
                                from: nil,
                                for: nil
                            )
                            openTools?(nil)
                        } label: {
                            Image(systemName: "square.grid.2x2")
                        }
                        .accessibilityLabel("ツールを開く")
                        .accessibilityIdentifier("task.tools")
                        if !model.isNewThread {
                            conversationActions
                        }
                    }
                }
            }
    }

    private var chatContent: some View {
        let history = model.history
        return VStack(spacing: 0) {
            ErrorNotices(notices: model.conversationContext.notices(), perform: { model.perform($0) })
                .padding(.horizontal)
            if let notice = model.notice, notice != history?.error?.plainText {
                AppNotice(text: notice).padding(.horizontal).padding(.top, 8)
            }
            if let thread = conversation {
                let rows = conversationRows(thread.rows, expansion: activityExpansionOverrides)
                let hasHistory = thread.id != nil && history != nil
                let ids: [ConversationList.RowID] = (hasHistory ? [.history] : []) + rows.map { .message($0.id) }
                ConversationList(ids: ids, scroll: scroll, following: scroll.following, request: scroll.request,
                                 accessibilityValue: threadAccessibilityValue(thread)) { index in
                    let row = index - (hasHistory ? 1 : 0)
                    return AnyView(Group {
                        if row < 0, let session = thread.id, let history {
                            historyControl(history, session: session)
                        } else if rows.indices.contains(row) {
                            conversationRow(rows[row])
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .buttonStyle(.plain)
                    .environment(\.locale, locale))
                }
                .id(model.draftKey)
                .overlay(alignment: .bottom) {
                    if !scroll.latestVisible, !rows.isEmpty {
                        Button {
                            scroll.goTo(.bottom)
                        } label: {
                            Image(systemName: "arrow.down").font(.title3.weight(.medium))
                                .frame(width: 44, height: 44)
                        }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.circle)
                        .accessibilityLabel("最新のメッセージへ")
                        .accessibilityIdentifier("task.latest")
                        .padding(.bottom, 6)
                    }
                }
            } else if model.isNewThread {
                ScrollView {
                    newThreadContext
                        .padding(.horizontal, 24)
                        .frame(maxWidth: .infinity)
                }
                .defaultScrollAnchor(.center, for: .alignment)
                .padding(.vertical, 24)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("task.empty")
            } else if let id = model.selectedThreadId, let error = history?.error {
                AppNotice(text: error).padding(.horizontal)
                Button("再試行") { model.openThread(id) }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("task.retry")
            } else {
                ProgressView("タスクを読み込み中…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("task.loading")
            }
        }
        .background(Color(UIColor.systemBackground))
        .safeAreaInset(edge: .bottom, spacing: 0) {
            ConversationComposer(model: model, mediaPresentation: mediaPresentation,
                                 hasConversation: conversation != nil,
                                 isSideChat: isSideChat, openTools: openTools)
        }
    }
}

/// Conversation navigation
extension ThreadScreen {
    private var project: ProjectSummary? {
        let directory = model.selectedDirectory
        return environment.list?.projects.first { $0.roots.contains { $0.path == directory } }
    }

    var conversationTitle: some View {
        let directory = model.selectedDirectory
        return VStack(spacing: 2) {
            HStack(spacing: 6) {
                conversationTitleText(model.conversationContext.title(), fallback: model.isNewThread ? "チャット" : "タスク")
                    .font(.headline).lineLimit(1)
                if environment.connecting {
                    ProgressView().controlSize(.small)
                        .tint(.red)
                        .accessibilityLabel("接続中")
                        .accessibilityIdentifier("connection.progress")
                }
            }
            Text([
                project?.name ?? (directory.isEmpty ? "" : URL(fileURLWithPath: directory).lastPathComponent),
                environment.hostName ?? "Mac"
            ].filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.caption).foregroundStyle(.secondary).lineLimit(1)
        }
        .frame(maxWidth: .infinity)
    }

    var conversationActions: some View {
        Group {
            Button { model.openNewThread(cwd: model.selectedDirectory) } label: {
                Image(systemName: "square.and.pencil")
            }.accessibilityLabel("新しい会話").accessibilityIdentifier("task.new")
            Menu {
                Button { openTools?(.diff) } label: {
                    Label("変更を表示", systemImage: "plus.forwardslash.minus")
                }
                Button {
                    if let id = model.selectedThreadId {
                        model.openThread(id)
                    }
                } label: { Label("更新", systemImage: "arrow.clockwise") }
            } label: {
                Image(systemName: "ellipsis")
            }
            .accessibilityLabel("その他")
            .accessibilityIdentifier("task.more")
        }
        .buttonStyle(.plain)
    }

    private var newThreadContext: some View {
        let directory = model.selectedDirectory
        let emptyFolderLabel = localizedUiText(language: locale.appLanguage, source: "チャット")
        return VStack(spacing: 24) {
            VStack(spacing: 8) {
                Text("何を作りましょうか？")
                    .accessibilityIdentifier("task.prompt")
                Menu {
                    Button { model.openNewThread(cwd: "") } label: {
                        Label("チャット", systemImage: "bubble.left.and.bubble.right")
                    }
                    ForEach(environment.list?.projects ?? [], id: \.id) { project in
                        ForEach(project.roots, id: \.path) { root in
                            Button { model.openNewThread(cwd: root.path) } label: {
                                Label(project.roots.count == 1 ? project.name : root.path,
                                      systemImage: root.path == directory ? "checkmark" : "folder")
                            }
                        }
                    }
                    if environment.list?.hasMoreProjects == true {
                        Button("さらにプロジェクトを読み込む") { model.perform(.expandProjects) }
                    }
                } label: {
                    Text(project?
                        .name ??
                        (directory.isEmpty ? emptyFolderLabel : URL(fileURLWithPath: directory).lastPathComponent))
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .frame(minWidth: 44, minHeight: 44)
                }
                .accessibilityLabel(
                    "フォルダ: \(project?.name ?? (directory.isEmpty ? emptyFolderLabel : directory))"
                )
                .accessibilityIdentifier("task.folder")
            }
            .font(.title2)
            Menu {
                ForEach(environment.profiles, id: \.id) { profile in
                    Button { environment.selectHost?(profile.id) } label: {
                        if profile.id == model.host {
                            Label(profile.name, systemImage: "checkmark")
                        } else {
                            Text(profile.name)
                        }
                    }
                }
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "laptopcomputer")
                    Text(environment.hostName ?? localizedUiText(language: locale.appLanguage, source: "環境を選択"))
                        .lineLimit(1).truncationMode(.middle)
                    if environment.connecting {
                        ProgressView().controlSize(.small).tint(.red)
                    }
                }
                .frame(minHeight: 44)
            }
            .font(.subheadline)
            .foregroundColor(.secondary)
            .accessibilityLabel(
                "環境: \(environment.hostName ?? localizedUiText(language: locale.appLanguage, source: "未選択"))"
            )
            .accessibilityIdentifier("task.environment")
        }
        .multilineTextAlignment(.center)
        .buttonStyle(.plain)
        .frame(maxWidth: .infinity)
        .disabled(model.transferring || model.sending || model.transcribing)
    }
}

private func conversationTitleText(_ title: String?, fallback: LocalizedStringKey) -> Text {
    title.map { Text(verbatim: $0) } ?? Text(fallback)
}
