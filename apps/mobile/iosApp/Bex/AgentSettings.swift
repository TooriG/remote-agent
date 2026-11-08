import AgentCore
import SwiftUI

struct ProviderAccountScreen: View {
    @Environment(\.locale) private var locale
    let model: ConversationModel
    var profiles: [HostProfile] = []
    var selectProfile: ((String) -> Void)?
    let provider: ProviderKind
    let close: () -> Void
    @State private var changingAccount = false
    @State private var loadingAccounts = false
    @State private var credentialProvider = KeyProvider.deepSeek
    @State private var loginCode = ""
    @State private var loginRequestInFlight = false
    @State private var cancellingLogin = false
    @State private var signOutId: String?
    private var accounts: [Account] {
        model.accounts.filter { $0.provider == provider }
    }

    private var agent: ConnectionAgent? {
        model.settings.connectionSetup().agents.first { $0.provider == provider }
    }

    private var login: AccountLogin? {
        model.settings.accountLogin()
    }

    /// Core records failures of starting, confirming, answering, and cancelling the login.
    private var loginError: ErrorText? {
        model.settings.accountLoginError(provider: provider)
    }

    private var loginInProgress: Bool {
        loginRequestInFlight || cancellingLogin || login != nil
    }

    private var busy: Bool {
        changingAccount || loginInProgress || !model.isConnected
    }

    private var loginProgressMessage: String? {
        if cancellingLogin {
            return "サインインを中止中…"
        }
        guard loginRequestInFlight else { return nil }
        return login == nil ? "サインインを準備中…" : "認証を確認中…"
    }

    var body: some View {
        List {
            Section("適用範囲") {
                if let selectProfile {
                    SettingsScopeFields(profiles: profiles, selectedProfileId: model.host,
                                        disabled: changingAccount || loginInProgress, select: selectProfile)
                } else {
                    Text(model.settings.hostName() ?? model.host)
                }
            }
            if let login {
                AccountLoginSection(
                    login: login, providerName: provider.bexName, loginCode: $loginCode,
                    progressMessage: loginProgressMessage, loginError: loginError,
                    submit: { submitLoginCode(login.loginId, provider: login.provider) },
                    retry: { readLogin(login) }
                )
            } else {
                Section {
                    if let agent {
                        Label(
                            agent.label,
                            systemImage: agent.availability == .ready ? "checkmark.circle.fill" : "circle"
                        )
                        .font(.caption).foregroundStyle(agent.availability == .ready ? .green : .secondary)
                        if let error = agent.error {
                            ErrorTextView(text: error).font(.caption).foregroundStyle(.red)
                        }
                    }
                }
                if provider == .deepSeek {
                    credentialSection
                } else {
                    accountSection
                }
                ProviderRuntimeSection(model: model, provider: provider)
            }
        }
        .navigationTitle(provider.bexName)
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(loginInProgress)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                if loginInProgress {
                    Button("キャンセル", action: cancelLogin)
                        .disabled(cancellingLogin)
                        .accessibilityIdentifier("model.back")
                }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("完了", action: close).disabled(loginInProgress)
                    .accessibilityIdentifier("model.close")
            }
        }
        .alert("サインアウトしますか？", isPresented: Binding(get: { signOutId != nil }, set: {
            if !$0 {
                signOutId = nil
            }
        })) {
            if let id = signOutId {
                Button("サインアウト", role: .destructive) { signOut(id) }
                    .accessibilityIdentifier("account.logout.confirm")
            }
            Button("キャンセル", role: .cancel) { signOutId = nil }
                .accessibilityIdentifier("account.logout.cancel")
        } message: {
            Text("この環境に保存されたアカウントからサインアウトします。再び使うにはサインインが必要です。")
        }
        .interactiveDismissDisabled(loginInProgress)
        .onAppear(perform: refresh)
        .onChange(of: model.isConnected) { connected in
            if connected {
                refresh()
            }
        }
        .onChange(of: model.host) { _ in
            loginCode = ""
            signOutId = nil
            if model.isConnected {
                refresh()
            }
        }
    }
}

extension ProviderAccountScreen {
    private var credentialSection: some View {
        let controls = model.settings.credentialControls()
        let selected = controls.first { $0.provider == credentialProvider }
        let updating = model.settings.credentialBusy()
        return Section("接続先のAPIキー") {
            Picker("接続先", selection: $credentialProvider) {
                ForEach(controls, id: \.provider) { route in Text(route.label).tag(route.provider) }
            }.onChange(of: credentialProvider) { _ in loginCode = "" }
            if let selected {
                Text(verbatim: selected.status).font(.caption).foregroundStyle(.secondary)
                if let url = URL(string: selected.keyUrl) {
                    Link("APIキーを取得", destination: url)
                }
                SecureField("APIキー", text: $loginCode).textInputAutocapitalization(.never).autocorrectionDisabled()
                Button("保存") {
                    let key = loginCode; loginCode = ""
                    model.perform(.updateDshCredential(KeyUpdate(provider: credentialProvider, key: key)))
                }
                .disabled(updating || loginCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !model
                    .isConnected)
                if selected.canRemove {
                    Button("この接続先のキーを削除", role: .destructive) {
                        model.perform(.updateDshCredential(KeyUpdate(provider: credentialProvider, key: nil)))
                    }.disabled(updating || !model.isConnected)
                }
            }
            Button("設定状態を取得", action: refresh).disabled(updating || !model.isConnected)
            if let error = model.settings.credentialError() {
                ErrorTextView(text: error).font(.caption).foregroundStyle(.red)
            }
        }
    }

    private var accountSection: some View {
        Section {
            ForEach(accounts, id: \.id) { account in
                let selected = model.settings.accountIsSelected(provider: account.provider, id: account.id)
                VStack(alignment: .leading, spacing: 12) {
                    HStack(spacing: 12) {
                        Button { chooseAccount(account.id) } label: {
                            HStack(spacing: 12) {
                                AccountIdentityView(account: account)
                                if let plan = account.planType, !plan.isEmpty {
                                    Text(plan.uppercased()).font(.caption2).foregroundStyle(.secondary)
                                }
                                Spacer(minLength: 0)
                                Image(systemName: selected
                                    ? "checkmark.circle.fill" : "circle").foregroundStyle(Color.accentColor)
                            }.foregroundStyle(.primary)
                        }
                        .accessibilityIdentifier("model.account." + account.id)
                        .accessibilityValue(selected ? "選択中" : "")
                        .disabled(busy)
                        Menu {
                            Button("サインアウト", role: .destructive) { signOutId = account.id }
                                .accessibilityIdentifier("account.logout." + account.id)
                        } label: { Image(systemName: "ellipsis").frame(width: 32, height: 44) }
                            .accessibilityLabel("アカウントの操作")
                            .accessibilityIdentifier("account.actions." + account.id)
                            .disabled(busy)
                    }
                    if selected {
                        AccountUsageView(usage: account.usage)
                            .accessibilityIdentifier("account.usage." + account.id)
                    }
                }
            }
            if accounts.isEmpty, !loadingAccounts, agent?.error == nil {
                Text(verbatim: localizedUiFormat(
                    language: locale.appLanguage,
                    source: "{0} にサインインして利用を開始できます。",
                    arguments: [provider.bexName]
                ))
                .foregroundStyle(.secondary)
            }
            if loadingAccounts {
                ProgressView("アカウントを更新中…")
            }
            if let loginProgressMessage {
                ProgressView(loginProgressMessage)
            }
            if let error = loginError {
                ErrorTextView(text: error).font(.caption).foregroundStyle(.red)
            }
        } header: {
            HStack {
                Text("アカウント")
                Spacer()
                Button("追加", systemImage: "plus", action: startLogin)
                    .accessibilityIdentifier("model.account.add").disabled(busy)
                Button(action: refresh) { Image(systemName: "arrow.clockwise") }
                    .accessibilityLabel("モデルと使用量を更新")
                    .accessibilityIdentifier("account.refresh")
                    .disabled(loadingAccounts || busy)
            }
        } footer: {
            Text(verbatim: localizedUiFormat(
                language: locale.appLanguage,
                source: "この環境の{0}で使用します。同じ環境を使う端末にも反映されます。",
                arguments: [provider.bexName]
            ))
        }
        .buttonStyle(.borderless)
    }

    private func chooseAccount(_ id: String) {
        changingAccount = true
        model.perform(.selectAccount(SelectAccount(provider: provider, id: id))) { _ in changingAccount = false }
    }

    private func refresh() {
        if provider == .deepSeek {
            model.perform(.readDshCredentials(ReadCredentials()))
        }
        // Core confirms a pending login; accounts are listed once it finishes.
        if login != nil {
            return
        }
        loadingAccounts = true
        model.perform(.listAccounts(ListAccounts())) { _ in loadingAccounts = false }
        model.perform(.refreshModels)
    }

    private func signOut(_ id: String) {
        signOutId = nil
        changingAccount = true
        model.perform(.logoutAccount(LogoutAccount(provider: provider, id: id))) { _ in changingAccount = false }
    }

    private func startLogin() {
        loginCode = ""
        loginRequestInFlight = true
        model.perform(
            .startAccountLogin(StartAccountLogin(provider: provider, threadId: nil)),
            completion: finishLoginRequest
        )
    }

    private func submitLoginCode(_ id: String, provider: ProviderKind) {
        loginRequestInFlight = true
        let code = loginCode
        loginCode = ""
        model.perform(
            .submitAccountLogin(SubmitAccountLogin(provider: provider, id: id, code: code)),
            completion: finishLoginRequest
        )
    }

    private func finishLoginRequest(_: Result<Outcome, Error>) {
        loginRequestInFlight = false
        if cancellingLogin {
            cancelLogin()
        }
    }

    private func readLogin(_ login: AccountLogin) {
        loginRequestInFlight = true
        model.perform(.readAccountLogin(ReadAccountLogin(provider: login.provider, id: login.loginId))) { _ in
            loginRequestInFlight = false
        }
    }

    private func cancelLogin() {
        cancellingLogin = true
        loginCode = ""
        // Finish the in-flight start or code submission before cancelling on the Host.
        guard !loginRequestInFlight else { return }
        guard let login else {
            cancellingLogin = false; return
        }
        model.perform(.cancelAccountLogin(CancelAccountLogin(
            provider: login.provider,
            id: login.loginId
        ))) { _ in
            cancellingLogin = false
        }
    }
}

extension ProviderKind {
    var bexName: String {
        switch self {
        case .codex: "Codex"
        case .claude: "Claude Code"
        case .deepSeek: "DeepSeek Harness"
        }
    }

    var bexKey: String {
        switch self {
        case .codex: "codex"
        case .claude: "claude"
        case .deepSeek: "deepseek"
        }
    }

    var bexIcon: String {
        switch self {
        case .codex: "openai"
        case .claude: "claude"
        case .deepSeek: "deepseek"
        }
    }
}
