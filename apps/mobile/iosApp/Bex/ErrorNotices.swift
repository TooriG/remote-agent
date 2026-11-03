import AgentCore
import SwiftUI
import UIKit

struct ErrorNotices: View {
    @ObservedObject var model: AppViewModel
    let scope: ErrorScope

    var body: some View {
        let notices = model.snapshot.errorNotices(scope: scope)
        if !notices.isEmpty {
            VStack(spacing: 8) {
                ForEach(notices, id: \.id) { notice in
                    let hasDetails = notice.message.count > 180 || notice.message.components(separatedBy: "\n")
                        .count > 3
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: "exclamationmark.circle").foregroundStyle(.red)
                        VStack(alignment: .leading, spacing: 4) {
                            AppText(notice.context).font(.caption).foregroundStyle(.secondary)
                            AppText(notice.title).font(.subheadline.weight(.semibold))
                            AppText(notice.message).font(.subheadline).foregroundStyle(.secondary)
                                .textSelection(.enabled).lineLimit(hasDetails ? 3 : nil)
                            if hasDetails {
                                DisclosureGroup("詳細を表示") {
                                    AppText(notice.message).font(.caption).textSelection(.enabled)
                                }.font(.caption)
                            }
                            if let recovery = model.snapshot.errorRecovery(id: notice.id) {
                                Button(recovery.label) { model.perform(recovery.intent) }
                            }
                            if scope == .application, !model.snapshot.connected() {
                                Button("再接続") { model.connect() }.disabled(model.isConnecting)
                            }
                        }
                        Spacer(minLength: 0)
                        Button {
                            UIPasteboard.general.string = "\(notice.title)\n\(notice.message)"
                        } label: { Image(systemName: "doc.on.doc") }
                            .accessibilityLabel("エラーの詳細をコピー")
                        Button { model.perform(.dismissError(id: notice.id)) } label: {
                            Image(systemName: "xmark")
                        }
                        .accessibilityLabel("エラーを閉じる")
                    }
                    .buttonStyle(.borderless)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(.red.opacity(0.2)))
                    .accessibilityIdentifier("error.notice")
                }
            }
        }
    }
}
