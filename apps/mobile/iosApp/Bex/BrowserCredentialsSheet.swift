import AgentCore
import SwiftUI

struct BrowserCredentialsSheet: View {
    let credentials: BrowserCredentials
    let send: (BrowserAction) -> Void
    let close: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section("接続先のブラウザから取り込む") {
                    if credentials.profiles.isEmpty {
                        Text("取り込めるブラウザプロファイルがありません。")
                    }
                    ForEach(credentials.profiles, id: \.label) { profile in
                        VStack(alignment: .leading, spacing: 12) {
                            Text(profile.label)
                            Button("ログイン状態（Cookie）") { send(.importCookies(profile: profile.id)) }
                            Button("保存パスワード → Keychain") { send(.importPasswords(profile: profile.id)) }
                        }.padding(.vertical, 4)
                    }
                }
                if !credentials.importErrors.isEmpty {
                    Section("読み取れなかったブラウザ") {
                        ForEach(credentials.importErrors, id: \.browser) { error in
                            VStack(alignment: .leading, spacing: 4) {
                                Text(error.browser)
                                AppText(error.message).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                if let origin = credentials.origin {
                    Section("保存パスワード · \(origin)") {
                        if credentials.accounts.isEmpty {
                            AppText(credentials.passwordError ?? "このサイトの保存パスワードはありません。")
                        }
                        ForEach(credentials.accounts, id: \.self) { username in
                            Button(username.isEmpty ? "ユーザー名なし" : username) {
                                send(.fillPassword(origin: origin, username: username))
                            }
                        }
                        Button("このサイトの保存パスワードを削除", role: .destructive) {
                            send(.deletePasswords(origin: origin))
                        }.disabled(credentials.accounts.isEmpty)
                    }
                }
            }
            .navigationTitle("ブラウザデータ").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("閉じる", action: close) } }
        }
    }
}
