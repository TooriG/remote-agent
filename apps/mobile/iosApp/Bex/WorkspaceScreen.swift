import AgentCore
import SwiftUI

private struct WorkspaceDestination: Hashable {
    let scope: PanelScope
    let directory: String
}

/// Keep the workbench on its conversation while side-chat operations change Store selection.
struct ConversationDestination: View {
    @ObservedObject var model: AppViewModel
    @State private var tools: WorkspaceDestination?

    var body: some View {
        GeometryReader { geometry in
            ThreadScreen(model: model,
                         conversation: model.isShowingSideChat ? model.sideChatRequest?.originalConversation : model
                             .conversation,
                         openTools: openTools)
                .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in
                    guard value.startLocation.x >= geometry.size.width - 32,
                          value.translation.width < -60,
                          abs(value.translation.width) > abs(value.translation.height) * 1.5 else { return }
                    openTools(nil)
                })
        }
        .navigationDestination(item: $tools) { destination in
            WorkspaceToolsScreen(model: model, scope: destination.scope, directory: destination.directory) {
                tools = nil
            }
        }
    }

    private func openTools(_ kind: PanelKind?) {
        let sideChat = model.isShowingSideChat ? model.sideChatRequest : nil
        let draft = sideChat.map { DraftKey.session(session: $0.originalThreadId) } ?? model.coreDraftKey
        let destination = WorkspaceDestination(
            scope: model.snapshot.panelScope(host: model.selectedProfileId ?? "", draft: draft),
            directory: sideChat?.cwd ?? model.cwd
        )
        if let kind {
            model.openWorkbench(kind, scope: destination.scope, directory: destination.directory)
        }
        tools = destination
    }
}

extension PanelKind {
    var icon: String {
        switch self {
        case .browser: "globe"
        case .terminal: "terminal"
        case .files, .file: "folder"
        case .diff: "arrow.left.arrow.right"
        case .sideChat: "bubble.left.and.bubble.right"
        case .agents: "network"
        case .home: "plus"
        }
    }
}

extension AppViewModel {
    func migrateWorkbench(to next: AgentCore.Snapshot) {
        guard let session = next.submittedSession(draft: coreDraftKey) else { return }
        let host = selectedProfileId ?? ""
        panels.rekey(before: snapshot.panelScope(host: host, draft: coreDraftKey),
                     after: next.panelScope(host: host, draft: .session(session: session)))
    }

    func openWorkbench(_ kind: PanelKind, scope: PanelScope, directory: String) {
        objectWillChange.send()
        let id = panels.open(key: scope, kind: kind, path: nil)
        if kind == .sideChat, case let .session(original) = scope.draft {
            let host = scope.host
            let parent: ConversationPresentation? = if let request = sideChatRequest,
                                                       request.host == host, request.originalThreadId == original {
                request.originalConversation
            } else {
                conversation
            }
            let request: SideChatRequest = if let pending = sideChatRequest, pending.threadId == nil,
                                              pending.host == host, pending.originalThreadId == original,
                                              !workbenchSideChats.values.contains(where: { $0.id == pending.id }) {
                pending
            } else {
                SideChatRequest(text: "", host: host, originalThreadId: original,
                                originalConversation: parent, cwd: directory)
            }
            workbenchSideChats[id] = request
        }
    }

    func activateWorkbench(_ id: UInt64, scope: PanelScope) {
        objectWillChange.send()
        _ = panels.activate(key: scope, id: id)
    }

    func closeWorkbench(_ surface: PanelSurface, scope: PanelScope) {
        objectWillChange.send()
        panels.close(key: scope, id: surface.id)
        if surface.kind == .terminal {
            perform(.killTerminal(KillTerminal(handle: workbenchTerminalHandle(surface.id))))
        }
        workbenchSideChats.removeValue(forKey: surface.id)
    }

    func workbenchTerminalHandle(_ id: UInt64) -> String {
        "agent-terminal-\(workbenchInstance)-\(id)"
    }
}

private struct WorkspaceToolsScreen: View {
    @ObservedObject var model: AppViewModel
    let scope: PanelScope
    let directory: String
    let close: () -> Void

    private var tools: [PanelTool] {
        panelTools(connected: model.isConnected, hasDirectory: !directory.isEmpty)
    }

    private var panel: PanelState {
        model.panels.get(key: scope)
    }

    private var selected: PanelSurface? {
        panel.surfaces.first { $0.id == panel.active }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        ForEach(panel.surfaces, id: \.id) { surface in
                            HStack(spacing: 0) {
                                Button { model.activateWorkbench(surface.id, scope: scope) } label: {
                                    Label(panelTitle(kind: surface.kind), systemImage: surface.kind.icon)
                                        .lineLimit(1).padding(.leading, 10).padding(.trailing, 6)
                                        .frame(minHeight: 44)
                                }
                                .accessibilityIdentifier("workbench.tab.\(surface.kind).\(surface.id)")
                                .accessibilityAddTraits(surface.id == panel.active ? [.isSelected] : [])
                                Button { model.closeWorkbench(surface, scope: scope) } label: {
                                    Image(systemName: "xmark").font(.caption2).frame(width: 36, height: 44)
                                }
                                .accessibilityLabel("タブを閉じる")
                                .accessibilityIdentifier("workbench.close.\(surface.id)")
                            }
                            .background(surface.id == panel.active ? Color.primary.opacity(0.08) : .clear,
                                        in: RoundedRectangle(cornerRadius: 6))
                        }
                        Menu {
                            ForEach(tools, id: \.key) { tool in
                                toolButton(tool)
                            }
                        } label: {
                            Image(systemName: "plus").frame(width: 44, height: 44)
                        }
                        .accessibilityLabel("機能を開く")
                        .accessibilityIdentifier("workbench.add")
                    }
                }
                Button(action: close) {
                    Image(systemName: "sidebar.right").frame(width: 44, height: 44)
                }
                .accessibilityLabel("会話に戻る")
                .accessibilityIdentifier("workbench.close")
            }
            .font(.system(size: 14)).foregroundStyle(.secondary).buttonStyle(.plain)
            Divider().opacity(0.6)
            Group {
                if let selected {
                    content(selected).id(selected.id)
                } else {
                    VStack(spacing: 4) {
                        Text("機能を開く").font(.system(size: 14)).padding(.bottom, 12)
                        ForEach(tools, id: \.key) { tool in
                            toolButton(tool)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .frame(minHeight: 44)
                        }
                    }
                    .buttonStyle(.plain).font(.system(size: 14))
                    .frame(maxWidth: 300).padding(24)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("workbench.launcher")
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(Color(uiColor: .systemBackground))
        .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in
            guard value.startLocation.x < 32, value.translation.width > 60,
                  abs(value.translation.width) > abs(value.translation.height) * 1.5 else { return }
            close()
        })
        .toolbar(.hidden, for: .navigationBar)
    }

    private func toolButton(_ tool: PanelTool) -> some View {
        Button { model.openWorkbench(tool.kind, scope: scope, directory: directory) } label: {
            Label(tool.label, systemImage: tool.kind.icon)
        }
        .disabled(!tool.enabled)
        .accessibilityIdentifier("workbench.\(tool.key)")
    }

    @ViewBuilder
    private func content(_ surface: PanelSurface) -> some View {
        switch surface.kind {
        case .sideChat:
            if let request = model.workbenchSideChats[surface.id] {
                ConversationSideChat(model: model, request: request)
            } else {
                Text("会話を始めるとサイドチャットを利用できます。")
                    .foregroundStyle(.secondary).padding()
            }
        case .browser:
            if case let .session(thread) = scope.draft, let store = model.store {
                WorkspaceBrowserScreen(store: store, thread: thread)
            } else {
                Text("会話を始めると、AIと同じブラウザを利用できます。")
                    .foregroundStyle(.secondary).padding()
            }
        case .terminal:
            TerminalScreen(model: model, cwd: directory, handle: model.workbenchTerminalHandle(surface.id))
        case .files, .file:
            WorkspaceScreen(model: model, root: directory, draft: scope.draft, close: close)
        case .diff:
            WorkspaceDiffScreen(model: model, root: directory)
        case .agents:
            WorkspaceAgentsScreen(agents: model.snapshot.agentPanel())
        case .home:
            EmptyView()
        }
    }
}
