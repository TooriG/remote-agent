import AgentCore
import SwiftUI

struct ConnectionSettingsRow: View {
    @Environment(\.locale) private var locale
    let model: AppViewModel
    let profile: HostProfile

    var body: some View {
        let state = model.connectionState(profile.id)
        HStack(spacing: 16) {
            NavigationLink {
                EnvironmentSettingsScreen(model: model, profile: profile)
            } label: {
                VStack(alignment: .leading, spacing: 6) {
                    Label(profile.name, systemImage: "desktopcomputer").fontWeight(.semibold)
                    Text(verbatim: connectionStatusLabel(
                        language: locale.appLanguage,
                        connected: state.connected,
                        enabled: state.enabled
                    )).font(.subheadline).foregroundStyle(.secondary)
                }
            }
            .accessibilityIdentifier("profiles.\(profile.id)")
            Toggle("接続", isOn: Binding(
                get: { state.enabled },
                set: { model.setConnectionEnabled(profile.id, enabled: $0) }
            ))
            .labelsHidden()
            .tint(.blue)
            .accessibilityLabel(localizedUiFormat(
                language: locale.appLanguage,
                source: "{0}への接続",
                arguments: [profile.name]
            ))
            .accessibilityIdentifier("connection.enabled.\(profile.id)")
        }
        .buttonStyle(.borderless)
        .padding(.vertical, 6)
    }
}

struct EnvironmentSettingsScreen: View {
    @Environment(\.locale) private var locale
    let model: AppViewModel
    let profile: HostProfile

    var body: some View {
        Form {
            ErrorNotices(notices: model.notices[.settings] ?? [], perform: { model.perform($0) })
            Section("接続") {
                Toggle(isOn: Binding(
                    get: { model.connectionState(profile.id).enabled },
                    set: { model.setConnectionEnabled(profile.id, enabled: $0) }
                )) {
                    VStack(alignment: .leading, spacing: 6) {
                        Label(profile.name, systemImage: "desktopcomputer").fontWeight(.semibold)
                        let state = model.connectionState(profile.id)
                        Text(verbatim: connectionStatusLabel(
                            language: locale.appLanguage,
                            connected: state.connected,
                            enabled: state.enabled
                        ))
                        .font(.subheadline).foregroundStyle(.secondary)
                    }
                }
                .accessibilityIdentifier("environment.enabled")
            }
            Section {
                DisclosureGroup("バージョン情報") {
                    if let software = model.settings.hostSoftware() {
                        LabeledContent("バージョン") { Text(software.version) }
                        if software.build != 0 {
                            LabeledContent("ビルド") { Text("\(software.build)") }
                        }
                        Text(software.revision).font(.caption.monospaced()).textSelection(.enabled)
                    } else {
                        Button("再読み込み") { model.perform(.readHostSoftware(ReadHostSoftware())) }
                            .disabled(!model.isConnected)
                    }
                }
                if let url = URL(string: "https://github.com/ttizze/remote-agent/releases") {
                    Link(destination: url) {
                        Label("アップデートを確認", systemImage: "arrow.clockwise")
                    }
                }
            } header: {
                Text(verbatim: "Bex Host")
            } footer: {
                Text("更新はこのPCでBexを開いて適用できます。")
            }
            ProviderSettingsSection(model: model)
        }
        .tint(.blue)
        .navigationTitle(profile.name)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { model.activateProfile(profile.id) }
        .task(id: "\(model.selectedProfileId ?? ""):\(model.isConnected)") {
            if model.selectedProfileId == profile.id, model.isConnected {
                model.perform(.readHostSoftware(ReadHostSoftware()))
            }
        }
    }
}

struct ProviderSettingsScreen: View {
    let model: AppViewModel

    var body: some View {
        Form {
            Section("適用範囲") {
                SettingsScopeFields(
                    profiles: model.profiles.map { SettingsScopeItem(id: $0.id, name: $0.name) },
                    selectedProfileId: model.selectedProfileId,
                    disabled: model.settings.accountLogin() != nil,
                    select: model.activateProfile
                )
            }
            ErrorNotices(notices: model.notices[.settings] ?? [], perform: { model.perform($0) })
            ProviderSettingsSection(model: model)
        }
        .tint(.blue)
        .navigationTitle("プロバイダー")
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct ProviderSettingsSection: View {
    @Environment(\.locale) private var locale
    let model: AppViewModel

    var body: some View {
        Section {
            Button(action: refresh) {
                Label("プロバイダーを再読み込み", systemImage: "arrow.clockwise")
            }
            .disabled(!model.isConnected)
            .accessibilityIdentifier("providers.refresh")
            ForEach(model.settings.connectionSetup().agents, id: \.provider) { provider in
                HStack(spacing: 16) {
                    NavigationLink {
                        if let chat = model.chat {
                            ProviderAccountScreen(model: chat, profiles: model.profiles,
                                                  selectProfile: model.activateProfile,
                                                  provider: provider.provider, close: model.showThreadList)
                                .id(chat.host)
                        }
                    } label: {
                        HStack(spacing: 12) {
                            Image(provider.provider.bexIcon).resizable().scaledToFit().frame(width: 24, height: 24)
                            VStack(alignment: .leading, spacing: 6) {
                                Text(provider.name).fontWeight(.semibold)
                                Text(verbatim: provider.label).font(.subheadline).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .accessibilityIdentifier("provider.\(provider.provider.bexKey)")
                    Toggle("プロバイダー", isOn: Binding(
                        get: { provider.enabled },
                        set: { model.perform(.setProviderEnabled(SetProviderEnabled(
                            provider: provider.provider,
                            enabled: $0
                        ))) }
                    ))
                    .labelsHidden()
                    .accessibilityLabel(localizedUiFormat(
                        language: locale.appLanguage,
                        source: "{0}を使用",
                        arguments: [provider.name]
                    ))
                    .accessibilityIdentifier("provider.enabled.\(provider.provider.bexKey)")
                    .disabled(!model.isConnected || !model.settings.providerSettingsLoaded() ||
                        model.settings.providerSettingsBusy())
                }
                .buttonStyle(.borderless)
                .padding(.vertical, 6)
            }
        } header: {
            Text("プロバイダー")
        } footer: {
            Text("選択したPCに適用され、同じ環境を使う端末にも反映されます。")
        }
        .task(id: "\(model.selectedProfileId ?? ""):\(model.isConnected)") {
            if model.isConnected {
                refresh()
            }
        }
    }

    private func refresh() {
        model.perform(.readProviderSettings(ReadProviderSettings()))
    }
}

struct ProviderRuntimeSection: View {
    let model: ConversationModel
    let provider: ProviderKind

    var body: some View {
        if provider == .codex {
            let status = model.settings.codexCliStatus()
            let updating = model.settings.codexCliBusy()
            Section {
                if let version = status?.version {
                    Text(version).font(.subheadline.monospaced())
                }
                Button("状態を確認") { model.perform(.manageCodexCli(ManageCodexCli(update: false))) }
                    .accessibilityIdentifier("provider.runtime.refresh")
                Button("最新版に更新") { model.perform(.manageCodexCli(ManageCodexCli(update: true))) }
                    .accessibilityIdentifier("provider.runtime.update")
                if updating {
                    ProgressView("確認・更新中…")
                }
                if status?.restartRequired == true {
                    Text("更新が完了しました。新しいCLIを使用するにはホストを再起動してください。")
                        .font(.footnote).foregroundStyle(.secondary)
                }
                if let error = status?.error {
                    ErrorTextView(source: error)
                }
            } header: {
                Text(verbatim: "Codex CLI")
            }
            .disabled(!model.isConnected || updating)
            .task(id: "\(model.host):\(model.isConnected)") {
                if model.isConnected {
                    model.perform(.manageCodexCli(ManageCodexCli(update: false)))
                }
            }
        } else {
            Section("ランタイム") {
                Text("このプロバイダーはBexと一緒に更新されます。")
                    .font(.footnote).foregroundStyle(.secondary)
            }
        }
    }
}
