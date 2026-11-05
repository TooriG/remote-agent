import AgentCore
import SwiftUI

struct ModelSettingsSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var model: AppViewModel

    var body: some View {
        NavigationStack {
            ModelSettingsScreen(model: model, close: { dismiss() })
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }
}

struct ModelSettingsScreen: View {
    @ObservedObject var model: AppViewModel
    let close: () -> Void
    @State private var search = ""
    @State private var providerOverride: ProviderKind?

    private var provider: ProviderKind? {
        providerOverride ?? model.snapshot.modelProviderForDraft(threadId: model.coreDraftKey)
    }

    private var disabled: Bool {
        !model.isConnected || model.sending
    }

    private var selectedAccount: Account? {
        model.accounts.first {
            $0.provider == provider && model.snapshot.accountIsSelected(provider: $0.provider, id: $0.id)
        }
    }

    var body: some View {
        Form {
            ErrorNotices(model: model, scope: .settings)
            Section("エージェント") {
                Picker("エージェント", selection: Binding<ProviderKind?>(
                    get: { provider }, set: {
                        if let value = $0 {
                            selectProvider(value)
                        }
                    }
                )) {
                    ForEach([ProviderKind.codex, .claude, .deepSeek], id: \.self) { value in
                        Text(value == .deepSeek ? "DeepSeek" : value.bexName).tag(Optional(value))
                    }
                }
                .pickerStyle(.segmented)
                .accessibilityIdentifier("model.provider")
                .disabled(disabled)
            }
            Section("モデル") { modelSection }
            quickControls
            Section {
                NavigationLink {
                    AgentSettingsScreen(model: model, provider: provider, close: close)
                } label: {
                    if provider == .deepSeek {
                        Label("APIキー", systemImage: "key")
                    } else {
                        AccountIdentityView(account: selectedAccount)
                    }
                }
                .accessibilityIdentifier("model.accounts.manage")
                if let account = selectedAccount {
                    WeeklyUsageView(windows: model.snapshot.accountWeeklyUsage(
                        provider: account.provider, id: account.id
                    ))
                    .accessibilityIdentifier("model.account.usage")
                }
            } header: {
                Text(LocalizedStringKey(provider == .deepSeek ? "接続" : "アカウント"))
            } footer: {
                Text("選んだモデルと応答設定は、次の新規チャットにも引き継ぎます。")
            }
        }
        .accessibilityIdentifier("model.picker")
        .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always), prompt: "モデルを検索")
        .navigationTitle("モデル設定")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("完了", action: close).accessibilityIdentifier("model.close")
            }
        }
        .onAppear {
            model.perform(.listAccounts(ListAccounts()))
            model.perform(.refreshModels)
        }
        .onChange(of: model.selectedProfileId) { _ in
            providerOverride = nil
            search = ""
        }
    }

    @ViewBuilder
    private var quickControls: some View {
        let controls = model.snapshot.modelQuickControls(threadId: model.coreDraftKey)
        if model.selectedModel?.provider == provider,
           !controls.efforts.isEmpty || controls.speedChoices.count > 1 {
            Section("応答") {
                if !controls.efforts.isEmpty {
                    Picker("思考の深さ", selection: Binding(get: { controls.effort }, set: model.chooseEffort)) {
                        ForEach(controls.efforts, id: \.self) { Text($0).tag($0) }
                    }
                    .pickerStyle(.menu)
                    .accessibilityIdentifier("model.sheet.effort")
                    .accessibilityValue(controls.effort)
                    .disabled(disabled)
                }
                if controls.speedChoices.count > 1 {
                    Picker("速度", selection: Binding(get: {
                        controls.speedChoices.first { $0.selected }?.id ?? "default"
                    }, set: model.chooseServiceTier)) {
                        ForEach(controls.speedChoices, id: \.id) { choice in
                            AppText(choice.label).tag(choice.id)
                        }
                    }
                    .pickerStyle(.menu)
                    .accessibilityIdentifier("model.sheet.fast")
                    .accessibilityValue(controls.fast ? "高速" : "通常")
                    .disabled(disabled)
                }
            }
        }
    }

    @ViewBuilder
    private var modelSection: some View {
        let choices = model.snapshot.modelsMatching(provider: provider, query: search)
        if choices.isEmpty {
            AppText(model.snapshot.modelsLoading(provider: provider) ? "モデルを読み込み中…" : "利用可能なモデルがありません")
                .foregroundStyle(.secondary)
        }
        ForEach(choices, id: \.model) { choice in
            Button { model.chooseModel(choice.model) } label: {
                ModelChoiceRow(label: choice.displayName, selected: model.selectedModel == choice.model)
            }
            .foregroundStyle(.primary)
            .accessibilityIdentifier("model.choice." + choice.id)
            .accessibilityValue(model.selectedModel == choice.model ? "選択中" : "")
            .disabled(disabled)
        }
        ForEach(model.snapshot.modelErrorMessages(provider: provider), id: \.self) { error in
            ErrorTextView(text: errorText(error: error)).font(.caption).foregroundStyle(.red)
                .accessibilityIdentifier("model.error")
        }
    }

    private func selectProvider(_ provider: ProviderKind) {
        providerOverride = provider
        search = ""
        if let choice = model.snapshot.modelForProvider(threadId: model.coreDraftKey, provider: provider) {
            model.chooseModel(choice)
        }
    }
}

struct ModelChoiceRow: View {
    let label: String
    let selected: Bool

    var body: some View {
        HStack {
            Text(label).fontWeight(selected ? .medium : .regular)
            Spacer()
            if selected {
                Image(systemName: "checkmark").foregroundStyle(Color.accentColor)
            }
        }
        .contentShape(Rectangle())
    }
}
