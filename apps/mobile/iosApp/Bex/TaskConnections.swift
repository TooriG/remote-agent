import SwiftUI

struct TaskConnectionEmptyState: View {
    let connect: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "laptopcomputer").font(.system(size: 48)).foregroundStyle(.secondary)
            Text("作業する環境を接続").font(.title2.bold())
            Text("接続すると、プロジェクトとタスクがここに表示されます")
                .foregroundStyle(.secondary).multilineTextAlignment(.center)
            Button("環境に接続", action: connect)
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("tasks.connect")
            Text("QRコード・接続情報を使う").font(.footnote).foregroundStyle(.secondary)
        }.padding(32)
    }
}

struct TaskConnectionsMenu: View {
    let profiles: [HostProfile]
    let selected: String?
    let all: Bool
    let connecting: Bool
    let loading: Bool
    let connected: Bool
    let select: (String?) -> Void

    var body: some View {
        Menu {
            Button { select(nil) } label: {
                Label("すべての接続先", systemImage: all ? "checkmark" : "square.stack")
            }.accessibilityIdentifier("tasks.connections.all")
            Divider()
            ForEach(profiles) { profile in
                Button { select(profile.id) } label: {
                    if !all, selected == profile.id {
                        Label(profile.name, systemImage: "checkmark")
                    } else {
                        Text(profile.name)
                    }
                }.accessibilityIdentifier("tasks.connections.\(profile.id)")
            }
        } label: {
            HStack(spacing: 5) {
                if connecting || loading {
                    ProgressView().controlSize(.mini)
                        .tint(connecting ? .red : .green)
                        .accessibilityLabel(LocalizedStringKey(connecting ? "接続中" : "読み込み中"))
                        .accessibilityIdentifier("connection.progress")
                } else {
                    Circle().fill(connected ? Color.blue : Color.red).frame(width: 6, height: 6)
                }
                Image(systemName: all ? "square.stack" : "laptopcomputer")
                if all {
                    Text("すべての接続先").lineLimit(1)
                } else {
                    Text(profiles.first { $0.id == selected }?.name ?? "PC Host").lineLimit(1)
                }
                Image(systemName: "chevron.down").font(.caption)
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            .accessibilityElement(children: .combine)
        }
        .accessibilityIdentifier("tasks.connections")
        .accessibilityValue(Text(LocalizedStringKey(connecting ? "接続中" : connected ? "接続済み" : "未接続")))
    }
}
