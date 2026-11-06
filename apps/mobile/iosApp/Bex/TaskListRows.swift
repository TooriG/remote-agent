import AgentCore
import SwiftUI

extension ThreadSummary {
    var listKey: String {
        "\(hostId):\(id.provider.bexKey):\(id.id)"
    }
}

extension View {
    func taskListRowStyle(leading: CGFloat = 20) -> some View {
        listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 0, leading: leading, bottom: 0, trailing: 20))
            .listRowBackground(Color.clear)
    }
}

struct ThreadListRow: View {
    let thread: ThreadSummary
    let model: AppViewModel
    var leadingInset: CGFloat = 20

    private var accessibilityID: String {
        "\(thread.id.provider.bexKey):\(thread.id.id)"
    }

    var body: some View {
        Button { model.openThread(thread.id, on: thread.hostId) } label: {
            HStack(spacing: 10) {
                Text(thread.title).font(.title3).foregroundColor(.primary).lineLimit(1)
                Spacer()
                if thread.waiting {
                    Image(systemName: TaskActivityAttributes.IconKind.waiting.symbol)
                        .foregroundStyle(.orange)
                        .accessibilityLabel("確認待ち")
                        .accessibilityIdentifier("tasks.waiting.\(accessibilityID)")
                } else if thread.active {
                    RunningIndicator().controlSize(.small)
                        .accessibilityIdentifier("tasks.running.\(accessibilityID)")
                } else if thread.unread {
                    Circle().fill(Color.primary).frame(width: 8, height: 8)
                        .accessibilityLabel("完了・未確認")
                        .accessibilityIdentifier("tasks.completed.\(accessibilityID)")
                }
                if let status = thread.worktreeStatus {
                    let unmerged = status == .unmerged
                    Image(unmerged ? "GitDiff" : "GitMerge")
                        .resizable()
                        .frame(width: 18, height: 18)
                        .foregroundStyle(unmerged ? .orange : .purple)
                        .accessibilityLabel(LocalizedStringKey(unmerged ? "main に未反映の変更あり" : "main にマージ済み"))
                        .accessibilityIdentifier(
                            "tasks.\(unmerged ? "unmerged" : "merged").\(accessibilityID)"
                        )
                }
            }
            .contentShape(Rectangle())
        }
        .contextMenu {
            Button(LocalizedStringKey(thread.settled ? "Active に戻す" : "片づける")) {
                model.perform(
                    .setSessionSettled(SetSessionSettled(threadId: thread.id, settled: !thread.settled)),
                    on: thread.hostId
                )
            }.disabled(!thread.settled && !thread.canSettle)
        }
        .accessibilityIdentifier("tasks.row.\(accessibilityID)")
        .accessibilityValue(LocalizedStringKey(
            thread.waiting ? "確認待ち" : thread.active ? "実行中" : thread.unread ? "完了・未確認" : ""
        ))
        .taskListRowStyle(leading: leadingInset)
    }
}

/// On iOS 26.6 devices a running row kept the indicator's layout space while the
/// indicator stayed stopped and invisible, and refreshing the unchanged row did not
/// restart it. A new indicator on each appearance and foreground return starts animating.
private struct RunningIndicator: View {
    @Environment(\.scenePhase) private var scenePhase
    @State private var generation = 0

    var body: some View {
        ProgressView()
            .id(generation)
            .onAppear { generation += 1 }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active {
                    generation += 1
                }
            }
    }
}

struct SettledThreadRows: View {
    let shelf: SettledThreads
    let model: AppViewModel
    var leadingInset: CGFloat = 20

    var body: some View {
        Button {
            model.perform(.setSettledExpanded(projectId: shelf.projectId, expanded: !shelf.expanded), on: shelf.hostId)
        } label: {
            Label("片づけ済み", systemImage: shelf.expanded ? "chevron.down" : "chevron.right")
        }
        .taskListRowStyle(leading: leadingInset)
        if shelf.expanded {
            ForEach(shelf.threads, id: \.listKey) { thread in
                ThreadListRow(thread: thread, model: model, leadingInset: leadingInset)
            }
            if shelf.loading {
                ProgressView().taskListRowStyle(leading: leadingInset)
            }
            if let error = shelf.error {
                AppNotice(text: error).taskListRowStyle(leading: leadingInset)
                Button("再試行") {
                    model.perform(.setSettledExpanded(projectId: shelf.projectId, expanded: true), on: shelf.hostId)
                }.taskListRowStyle(leading: leadingInset)
            }
            if shelf.hasMore {
                Button("もっと見る") {
                    model.perform(.expandSettled(projectId: shelf.projectId), on: shelf.hostId)
                }
                .disabled(shelf.loading)
                .taskListRowStyle(leading: leadingInset)
            }
        }
    }
}
