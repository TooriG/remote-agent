import AgentCore
import SwiftUI

struct ThreadsScreen: View {
    @ObservedObject var model: AppViewModel
    @State private var search = ""
    @State private var showingSettings = false

    var body: some View {
        let projects = model.list?.projects ?? []
        let chats = model.list?.threads ?? []
        List {
            ErrorNotices(model: model, scope: .projects)
            if let notice = model.list?.notice {
                AppNotice(text: notice)
            }
            if model.threadLoadState == .failed {
                Section {
                    Button("再試行") { model.refreshTaskList() }
                        .accessibilityIdentifier("tasks.retry")
                }
            }
            if let notice = model.notice {
                Section {
                    AppNotice(text: notice)
                }
            }

            Section {
                HStack {
                    Label(model.selectedProfileName ?? "PC Host", systemImage: "laptopcomputer")
                    Spacer()
                    if model.isConnecting || model.threadLoadState == .loading {
                        ProgressView().controlSize(.small)
                            .accessibilityIdentifier("connection.progress")
                    } else {
                        Text(model.isConnected ? "接続済み" : "未接続")
                            .foregroundStyle(.secondary)
                    }
                }
                .accessibilityElement(children: .combine)
                .accessibilityLabel(
                    "\(model.selectedProfileName ?? "PC Host")、\(model.isConnected ? "接続済み" : "未接続")"
                )
            }

            Section {
                ForEach(projects, id: \.id) { project in
                    let expanded = Binding(
                        get: { project.expanded },
                        set: { model.perform(.setProjectExpanded(projectId: project.id, expanded: $0)) }
                    )
                    DisclosureGroup(isExpanded: expanded) {
                        if project.loading, !project.hasMore {
                            ProgressView()
                        }
                        if let error = project.error {
                            AppNotice(text: error)
                            Button("再試行") { model.perform(.refreshProject(projectId: project.id)) }
                        }
                        ForEach(project.threads, id: \.id) { thread in
                            ThreadListRow(thread: thread) { model.openThread(thread.id) }
                        }
                        if project.hasMore {
                            Button { model.expandTaskList(projectId: project.id) } label: {
                                HStack {
                                    Text("もっと見る")
                                    if project.loading {
                                        ProgressView().controlSize(.small)
                                    }
                                }
                            }
                            .disabled(project.loading)
                            .accessibilityIdentifier("tasks.project.\(project.id).more")
                        }
                    } label: {
                        HStack {
                            // Identify the header control so expanded rows keep their accessibility identities.
                            Button { expanded.wrappedValue.toggle() } label: {
                                HStack {
                                    ProjectIcon(
                                        png: project.iconPng,
                                        monogram: project.monogram,
                                        colorRGB: project.iconColor
                                    )
                                    Text(project.name).foregroundStyle(.primary).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .frame(minHeight: 44).contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("tasks.project.\(project.id)")
                            .accessibilityValue(project.expanded ? "開いています" : "閉じています")
                            Button { model.openNewThread(cwd: project.roots.first?.path ?? "") } label: {
                                Image(systemName: "square.and.pencil")
                                    .frame(minWidth: 44, minHeight: 44)
                            }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("\(project.name)で新しいタスク")
                            .accessibilityIdentifier("tasks.new.project.\(project.id)")
                        }
                    }
                }
            } header: {
                Text("プロジェクト").accessibilityIdentifier("tasks.projects")
            }

            if model.list?.hasMoreProjects == true {
                Section {
                    Button {
                        model.perform(.expandProjects)
                    } label: {
                        HStack(spacing: 8) {
                            if model.list?.loadingProjects == true {
                                ProgressView().controlSize(.mini)
                            }
                            Text("もっとプロジェクトを表示")
                        }
                    }
                    .disabled(model.loadingThreads || model.list?.loadingProjects == true)
                    .accessibilityIdentifier("tasks.projects.more")
                }
            }

            if model.threadLoadState == .ready, (model.list?.projects ?? []).isEmpty {
                Section {
                    Text("登録されたプロジェクトはありません")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                }
            }

            Section {
                if model.threadLoadState == .ready, chats.isEmpty {
                    Text("チャットがありません")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                        .accessibilityIdentifier("tasks.empty")
                }
                ForEach(chats, id: \.id) { thread in
                    ThreadListRow(thread: thread) { model.openThread(thread.id) }
                }
                if model.list?.hasMore == true {
                    Button {
                        model.expandTaskList()
                    } label: {
                        HStack(spacing: 8) {
                            if model.list?.loadingChats == true {
                                ProgressView().controlSize(.mini)
                            }
                            Text("もっと見る")
                        }
                    }
                    .disabled(model.loadingThreads || model.list?.loadingChats == true)
                    .accessibilityIdentifier("tasks.chats.more")
                }
            } header: {
                Text("チャット")
            }
        }
        .onChange(of: model.threadLoadState) { _, state in
            if state == .ready {
                model.recordListViewUpdate()
            }
        }
        .onAppear {
            if model.threadLoadState == .ready {
                model.recordListViewUpdate()
            }
        }
        .listStyle(.insetGrouped)
        .accessibilityIdentifier("tasks.list")
        .searchable(text: $search, placement: .toolbar, prompt: "チャットを検索")
        .refreshable { model.refreshTaskList() }
        .task(id: search) {
            do { try await Task.sleep(nanoseconds: 200_000_000) } catch { return }
            model.searchTaskList(search)
        }
        .navigationTitle("タスク")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            DefaultToolbarItem(kind: .search, placement: .bottomBar)
            ToolbarSpacer(.flexible, placement: .bottomBar)
            ToolbarItem(placement: .bottomBar) {
                Button { model.openNewThread(cwd: "") } label: {
                    Image(systemName: "square.and.pencil")
                }
                .accessibilityLabel("プロジェクトなしで新しいタスク")
                .accessibilityIdentifier("tasks.new.chat")
            }
            ToolbarItem(placement: .navigationBarTrailing) {
                Menu {
                    Button { showingSettings = true } label: { Label("設定", systemImage: "gearshape") }
                        .accessibilityIdentifier("tasks.settings")
                    Button { model.refreshTaskList() } label: { Label("更新", systemImage: "arrow.clockwise") }
                        .disabled(model.threadLoadState == .loading)
                        .accessibilityIdentifier("tasks.refresh")
                } label: { Image(systemName: "ellipsis") }
                    .accessibilityLabel("その他")
                    .accessibilityIdentifier("tasks.menu")
            }
        }
        .sheet(isPresented: $showingSettings) { SettingsSheet(model: model) }
    }
}

private struct ProjectIcon: View {
    let png: Data?
    let monogram: String
    let colorRGB: UInt32

    var body: some View {
        let color = Color(
            red: Double((colorRGB >> 16) & 255) / 255,
            green: Double((colorRGB >> 8) & 255) / 255,
            blue: Double(colorRGB & 255) / 255
        )
        Group {
            if let png, let image = UIImage(data: png) {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                Text(monogram)
                    .font(.system(size: 10, weight: .bold, design: .rounded))
                    .foregroundStyle(color)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(color.opacity(0.15))
            }
        }
        .frame(width: 24, height: 24)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .accessibilityHidden(true)
    }
}

private struct ThreadListRow: View {
    let thread: ThreadSummary
    let open: () -> Void

    private var accessibilityID: String {
        "\(thread.id.provider.bexKey):\(thread.id.id)"
    }

    var body: some View {
        Button(action: open) {
            HStack(spacing: 10) {
                Text(thread.title).foregroundStyle(.primary).lineLimit(2)
                Spacer()
                if thread.active {
                    ProgressView().controlSize(.small)
                        .accessibilityIdentifier("tasks.running.\(accessibilityID)")
                } else if thread.unread {
                    Circle().fill(Color.accentColor).frame(width: 8, height: 8)
                        .accessibilityLabel("完了・未確認")
                        .accessibilityIdentifier("tasks.completed.\(accessibilityID)")
                }
                if let status = thread.worktreeStatus {
                    let unmerged = status == .unmerged
                    Image(unmerged ? "GitDiff" : "GitMerge")
                        .resizable()
                        .frame(width: 18, height: 18)
                        .foregroundStyle(unmerged ? .orange : .purple)
                        .accessibilityLabel(unmerged ? "main に未反映の変更あり" : "main にマージ済み")
                        .accessibilityIdentifier(
                            "tasks.\(unmerged ? "unmerged" : "merged").\(accessibilityID)"
                        )
                }
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold)).foregroundStyle(.tertiary)
            }
            .contentShape(Rectangle())
        }
        .accessibilityIdentifier("tasks.row.\(accessibilityID)")
        .accessibilityValue(thread.active ? "実行中" : thread.unread ? "完了・未確認" : "")
    }
}
