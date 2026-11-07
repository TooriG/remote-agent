import AgentCore
import SwiftUI
import UIKit

/// Error copy in the UI language above its verbatim cause.
struct ErrorTextView: View {
    @Environment(\.locale) private var locale
    private enum Input {
        case formatted(ErrorText)
        case source(String)
    }

    private let input: Input

    init(text: ErrorText) {
        input = .formatted(text)
    }

    init(source: String) {
        input = .source(source)
    }

    private var resolved: ErrorText {
        switch input {
        case let .formatted(text): text
        case let .source(source): errorText(language: locale.appLanguage, error: source)
        }
    }

    var body: some View {
        let text = resolved
        VStack(alignment: .leading, spacing: 2) {
            Text(verbatim: text.message)
            if let details = text.details {
                Text(verbatim: details).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
        }
    }
}

/// Flat error labels keep their existing native typography and layout.
struct ErrorMessage: View {
    @Environment(\.locale) private var locale
    let source: String
    var body: some View {
        Text(verbatim: localizedErrorMessage(language: locale.appLanguage, error: source))
    }
}

extension ErrorText {
    /// The copy, then the verbatim cause on its own line, for single-string surfaces.
    var plainText: String {
        [message, details].compactMap(\.self).joined(separator: "\n")
    }
}

struct ErrorNotices: View {
    let notices: [ErrorNotice]
    let perform: (Intent) -> Void
    var reconnect: (() -> Void)?
    var reconnecting = false

    var body: some View {
        if !notices.isEmpty {
            VStack(spacing: 8) {
                ForEach(notices, id: \.id) { notice in
                    let details = notice.text.details ?? ""
                    let hasDetails = details.count > 180 || details.components(separatedBy: "\n").count > 3
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: "exclamationmark.circle").foregroundStyle(.red)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(verbatim: notice.context).font(.caption).foregroundStyle(.secondary)
                            Text(verbatim: notice.title).font(.subheadline.weight(.semibold))
                            Text(verbatim: notice.text.message).font(.subheadline).foregroundStyle(.secondary)
                                .textSelection(.enabled)
                            if !details.isEmpty {
                                Text(verbatim: details).font(.caption).foregroundStyle(.secondary)
                                    .textSelection(.enabled).lineLimit(hasDetails ? 3 : nil)
                            }
                            if hasDetails {
                                DisclosureGroup("詳細を表示") {
                                    Text(verbatim: details).font(.caption).textSelection(.enabled)
                                }.font(.caption)
                            }
                            if let label = notice.recoveryLabel {
                                Button { perform(.recoverError(id: notice.id)) } label: { Text(verbatim: label) }
                            }
                            if let reconnect {
                                Button("再接続", action: reconnect).disabled(reconnecting)
                            }
                        }
                        Spacer(minLength: 0)
                        Button {
                            UIPasteboard.general.string = "\(notice.title)\n\(notice.text.plainText)"
                        } label: { Image(systemName: "doc.on.doc") }
                            .accessibilityLabel("エラーの詳細をコピー")
                        Button { perform(.dismissError(id: notice.id)) } label: {
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
