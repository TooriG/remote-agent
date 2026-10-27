import AgentCore
import SwiftUI

/// Native navigation and resources; the core owns tabs, selection and the launcher catalog.
struct ConversationDestination: View {
    @ObservedObject var model: BexAppViewModel
    @State private var showingTools = false

    var body: some View {
        GeometryReader { geometry in
            ThreadScreen(model: model,
                         conversation: model.isShowingSideChat ? model.sideChatRequest?.originalConversation : model
                             .conversation,
                         openTools: { kind in
                             if let kind {
                                 model.openWorkbench(kind)
                             }
                             showingTools = true
                         })
                         .simultaneousGesture(DragGesture(minimumDistance: 30).onEnded { value in
                             guard value.startLocation.x >= geometry.size.width - 32,
                                   value.translation.width < -60,
                                   abs(value.translation.width) > abs(value.translation.height) * 1.5 else { return }
                             showingTools = true
                         })
        }
        .navigationDestination(isPresented: $showingTools) {
            WorkspaceToolsScreen(model: model) { showingTools = false }
        }
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

extension BexAppViewModel {
    func migrateWorkbench(to next: AgentCore.Snapshot) {
        guard let session = next.submittedSession(draft: coreDraftKey) else { return }
        let host = selectedProfileId ?? ""
        panels.rekey(before: snapshot.panelScope(host: host, draft: coreDraftKey),
                     after: next.panelScope(host: host, draft: .session(session: session)))
    }

    var workbenchScope: PanelScope {
        let draft: DraftKey = if isShowingSideChat, let request = sideChatRequest {
            .session(session: request.originalThreadId)
        } else {
            coreDraftKey
        }
        return snapshot.panelScope(host: selectedProfileId ?? "", draft: draft)
    }

    var workbench: PanelState {
        panels.get(key: workbenchScope)
    }

    var workbenchTools: [PanelTool] {
        panelTools(connected: isConnected, hasDirectory: !cwd.isEmpty)
    }

    func openWorkbench(_ kind: PanelKind) {
        objectWillChange.send()
        let scope = workbenchScope
        let id = panels.open(key: scope, kind: kind, path: nil)
        if kind == .sideChat, let host = selectedProfileId,
           case let .session(original) = scope.draft {
            let request: SideChatRequest = if let pending = sideChatRequest, pending.threadId == nil,
                                              pending.host == host, pending.originalThreadId == original {
                pending
            } else {
                SideChatRequest(text: "", host: host, originalThreadId: original,
                                originalConversation: isShowingSideChat ? sideChatRequest?
                                    .originalConversation : conversation,
                                cwd: selectedDirectory)
            }
            workbenchSideChats[id] = request
        }
    }

    func activateWorkbench(_ id: UInt64) {
        objectWillChange.send()
        _ = panels.activate(key: workbenchScope, id: id)
    }

    func closeWorkbench(_ surface: PanelSurface) {
        objectWillChange.send()
        panels.close(key: workbenchScope, id: surface.id)
        if surface.kind == .terminal {
            perform(.killTerminal(KillTerminal(handle: workbenchTerminalHandle(surface.id))))
        }
        workbenchSideChats.removeValue(forKey: surface.id)
    }

    func workbenchTerminalHandle(_ id: UInt64) -> String {
        "bex-terminal-\(workbenchInstance)-\(id)"
    }
}

private struct WorkspaceToolsScreen: View {
    @ObservedObject var model: BexAppViewModel
    let close: () -> Void

    private var panel: PanelState {
        model.workbench
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
                                Button { model.activateWorkbench(surface.id) } label: {
                                    Label(panelTitle(kind: surface.kind), systemImage: surface.kind.icon)
                                        .lineLimit(1).padding(.leading, 10).padding(.trailing, 6)
                                        .frame(minHeight: 44)
                                }
                                .accessibilityIdentifier("workbench.tab.\(surface.kind).\(surface.id)")
                                .accessibilityAddTraits(surface.id == panel.active ? [.isSelected] : [])
                                Button { model.closeWorkbench(surface) } label: {
                                    Image(systemName: "xmark").font(.caption2).frame(width: 36, height: 44)
                                }
                                .accessibilityLabel("タブを閉じる")
                                .accessibilityIdentifier("workbench.close.\(surface.id)")
                            }
                            .background(surface.id == panel.active ? Color.primary.opacity(0.08) : .clear,
                                        in: RoundedRectangle(cornerRadius: 6))
                        }
                        Menu {
                            ForEach(model.workbenchTools, id: \.key) { tool in
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
                        ForEach(model.workbenchTools, id: \.key) { tool in
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
        Button { model.openWorkbench(tool.kind) } label: {
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
            if case let .session(thread) = model.workbenchScope.draft, let store = model.store {
                WorkspaceBrowserScreen(store: store, thread: thread)
            } else {
                Text("会話を始めると、AIと同じブラウザを利用できます。")
                    .foregroundStyle(.secondary).padding()
            }
        case .terminal:
            TerminalScreen(model: model, cwd: model.cwd, handle: model.workbenchTerminalHandle(surface.id))
        case .files, .file:
            WorkspaceScreen(model: model, root: model.cwd, close: close)
        case .diff:
            WorkspaceDiffScreen(model: model, root: model.cwd)
        case .agents:
            WorkspaceAgentsScreen(agents: model.snapshot.agentPanel())
        case .home:
            EmptyView()
        }
    }
}
