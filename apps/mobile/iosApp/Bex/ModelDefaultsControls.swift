import AgentCore
import SwiftUI

struct NewChatDefaultModelMenu: View {
    let selected: ModelRef?
    let choices: [AgentCore.Model]
    let disabled: Bool
    let select: (ModelRef?) -> Void

    var body: some View {
        let label = choices.first { $0.model == selected }?.displayName ?? selected?.id ?? "自動"
        Picker("新規チャットのモデル", selection: Binding<ModelRef?>(get: { selected }, set: select)) {
            Text("自動（利用可能なエージェントのデフォルト）").tag(ModelRef?.none)
                .accessibilityIdentifier("model.defaults.new-chat.automatic")
            ForEach(choices, id: \.model) { choice in
                Text(choice.model.provider.bexName + " · " + choice.displayName).tag(Optional(choice.model))
                    .accessibilityIdentifier("model.defaults.new-chat." + choice.id)
            }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("model.defaults.new-chat")
        .accessibilityValue(label)
        .disabled(disabled)
    }
}

struct ModelDefaultsScopeFields: View {
    let projects: [ModelScopeChoice]
    let environments: [ModelScopeChoice]
    let selected: ModelDefaultsScope
    let disabled: Bool
    let select: (ModelDefaultsScope) -> Void

    var body: some View {
        Group {
            picker("環境", choices: environments, id: "settings.scope.environment")
            picker("プロジェクト", choices: projects, id: "settings.scope.projects")
        }
        .disabled(disabled)
    }

    private func picker(_ title: String, choices: [ModelScopeChoice], id: String) -> some View {
        Picker(title, selection: Binding(get: { selected }, set: select)) {
            ForEach(choices, id: \.id) { choice in
                Text(choice.label).tag(choice.scope)
                    .accessibilityIdentifier(id + "." + choice.id)
            }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier(id)
    }
}
