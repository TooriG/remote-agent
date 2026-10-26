import AgentCore
import SwiftUI
import UIKit

/// Owns the transition from a conversation to its separate tool destination.
struct ConversationDestination: View {
    @ObservedObject var model: BexAppViewModel
    @State private var showingTools = false
    @State private var tab: WorkspaceTab = .terminal
    @State private var showingDiff = false

    var body: some View {
        GeometryReader { geometry in
            ThreadScreen(model: model,
                         conversation: model.isShowingSideChat ? model.sideChatRequest?.originalConversation : model
                             .conversation,
                         openTools: { tab, diff in
                             if let tab {
                                 self.tab = tab
                             }
                             showingDiff = diff
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
            WorkspaceToolsScreen(model: model, tab: $tab, showingDiff: $showingDiff) {
                showingTools = false
            }
        }
    }
}

enum WorkspaceTab: String, CaseIterable {
    case terminal, browser, files, sideChat

    var icon: String {
        switch self {
        case .sideChat: "bubble.left.and.bubble.right"
        case .terminal: "terminal"
        case .files: "folder"
        case .browser: "globe"
        }
    }

    var label: String {
        switch self {
        case .sideChat: "サイドチャット"
        case .terminal: "ターミナル"
        case .files: "ファイル"
        case .browser: "ブラウザ"
        }
    }
}

private struct WorkspaceToolsScreen: View {
    @ObservedObject var model: BexAppViewModel
    @Binding var tab: WorkspaceTab
    @Binding var showingDiff: Bool
    let close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                ForEach(WorkspaceTab.allCases, id: \.self) { tab in
                    Button { self.tab = tab } label: {
                        Image(systemName: tab.icon)
                            .font(.title3)
                            .foregroundStyle(self.tab == tab ? .primary : .secondary)
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .overlay(alignment: .bottom) {
                                if self.tab == tab {
                                    Rectangle().fill(.primary).frame(height: 2)
                                }
                            }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(tab.label)
                    .accessibilityIdentifier("workbench.\(tab.rawValue)")
                    .accessibilityAddTraits(self.tab == tab ? [.isSelected] : [])
                }
            }
            Divider()
            Group {
                if tab == .sideChat {
                    if let request = model.sideChatRequest,
                       request.host == model.selectedProfileId,
                       request.originalThreadId == model.selectedThreadId || model.isShowingSideChat {
                        ConversationSideChat(model: model, request: request)
                            .id(request.id)
                    } else {
                        Text("会話を始めるとサイドチャットを利用できます。")
                            .foregroundStyle(.secondary).padding()
                    }
                } else if tab == .browser {
                    if let thread = model.isShowingSideChat ? model.sideChatRequest?.originalThreadId : model
                        .selectedThreadId, let store = model.store {
                        WorkspaceBrowserScreen(store: store, thread: thread)
                            .id("\(model.selectedProfileId ?? "")/\(thread)/\(ObjectIdentifier(store))")
                    } else {
                        Text("会話を始めると、AIと同じブラウザを利用できます。")
                            .foregroundStyle(.secondary).padding()
                    }
                } else if model.cwd.isEmpty {
                    Text("フォルダを選択すると\(tab.label)を利用できます。")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if tab == .terminal {
                    TerminalScreen(model: model, cwd: model.cwd)
                        .id(model.cwd)
                } else {
                    WorkspaceScreen(model: model, root: model.cwd, showingDiff: $showingDiff, close: close)
                        .id(model.cwd)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .onAppear { prepareSideChat() }
        .onChange(of: tab) { _ in prepareSideChat() }
        .navigationTitle("")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func prepareSideChat() {
        guard tab == .sideChat else { return }
        if model.sideChatRequest?.host != model.selectedProfileId ||
            (model.sideChatRequest?.originalThreadId != model.selectedThreadId && !model.isShowingSideChat) {
            model.askSelectionInSideChat("")
        }
    }
}
