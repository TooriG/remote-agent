import AgentCore
import SwiftUI

struct ComposerMessageField: View {
    let model: ConversationModel
    @FocusState.Binding var composerFocused: Bool

    var body: some View {
        BufferedTextInput(value: Binding(get: { model.draft }, set: {
            model.perform(.editComposer(threadId: model.coreDraftKey, text: $0, cursor: UInt32($0.utf8.count)))
        })) { input in
            VStack(alignment: .leading, spacing: 8) {
                let suggestions = model.composer.composerSuggestions(
                    text: input.wrappedValue, cursor: UInt32(input.wrappedValue.utf8.count)
                )
                let candidates = suggestions?.candidates ?? []
                if let status = suggestions?.status {
                    AppText(status).font(.caption).foregroundStyle(.secondary)
                }
                if !candidates.isEmpty {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(candidates, id: \.invocation.path) { candidate in
                                Button {
                                    if let inserted = insertInvocation(
                                        text: input.wrappedValue, cursor: UInt32(input.wrappedValue.utf8.count),
                                        kind: candidate.invocation.kind, name: candidate.invocation.name
                                    ) {
                                        input.wrappedValue = inserted.text
                                        model.perform(.insertInvocation(
                                            threadId: model.coreDraftKey,
                                            text: inserted.text,
                                            invocation: candidate.invocation
                                        ))
                                        composerFocused = true
                                    }
                                } label: {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(candidate.invocation.name).font(.subheadline)
                                        Text(candidate.description).font(.caption).foregroundStyle(.secondary)
                                            .lineLimit(2)
                                    }.frame(maxWidth: .infinity, alignment: .leading).padding(8)
                                }
                                .accessibilityIdentifier("composer.invocation.\(candidate.invocation.name)")
                            }
                        }
                    }.frame(maxHeight: 180)
                }
                TextField(model.isNewThread ? "メッセージを入力" : "追加の指示を入力", text: input, axis: .vertical)
                    .lineLimit(1 ... 6)
                    .focused($composerFocused)
                    .accessibilityIdentifier("task.message")
            }
        }
        .id(model.draftKey)
    }
}

struct ComposerModelControls: View {
    let model: ConversationModel
    @FocusState.Binding var composerFocused: Bool
    @Binding var showingModelSettings: Bool

    var body: some View {
        let controls = model.composer.controls().quick
        if let next = controls.toggleFastTo {
            Button { model.chooseServiceTier(next) } label: {
                Image(systemName: controls.fast ? "bolt.fill" : "bolt")
                    .foregroundStyle(controls.fast ? Color.primary : .secondary)
                    .frame(width: 44, height: 44)
            }
            .accessibilityLabel("Fast")
            .accessibilityValue(controls.fast ? "オン" : "オフ")
            .accessibilityIdentifier("model.fast")
            .disabled(!model.isConnected || model.sending)
        }
        let modelName = model.currentModel?.displayName ?? "モデル"
        Button { composerFocused = false; showingModelSettings = true } label: {
            Text(modelName)
                .foregroundStyle(.primary)
                .font(.subheadline).lineLimit(1).truncationMode(.middle)
                .frame(minWidth: 44, minHeight: 44)
                .contentShape(Rectangle())
        }
        .layoutPriority(1)
        .accessibilityLabel("モデル設定")
        .accessibilityValue(modelName)
        .accessibilityIdentifier("model.settings")
        if !controls.efforts.isEmpty {
            Menu {
                ForEach(controls.efforts, id: \.self) { effort in
                    Button { model.chooseEffort(effort) } label: {
                        if effort == controls.effort {
                            Label(effort, systemImage: "checkmark")
                        } else {
                            Text(effort)
                        }
                    }
                    .accessibilityIdentifier("model.effort." + effort)
                }
            } label: {
                ReasoningStrengthIcon(level: controls.effortLevel, count: controls.efforts.count)
                    .frame(width: 44, height: 44)
            }
            .accessibilityLabel("推論の強度")
            .accessibilityValue(controls.effort)
            .accessibilityIdentifier("model.effort")
            .disabled(!model.isConnected || model.sending)
        }
    }
}

struct ComposerStopButton: View {
    let turnId: String
    let interrupting: Bool
    let stop: (String) -> Void

    var body: some View {
        Button { stop(turnId) } label: {
            Image(systemName: "stop.fill").font(.system(size: 15))
        }
        .buttonStyle(ComposerActionButtonStyle(tint: .red))
        .disabled(interrupting)
        .accessibilityLabel(LocalizedStringKey(interrupting ? "停止中" : "停止"))
        .accessibilityIdentifier("turn.interrupt.\(turnId)")
    }
}

struct ComposerActionButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    var tint: Color = .accentColor

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(isEnabled ? Color.white : Color.primary)
            .frame(width: 32, height: 32)
            .background(isEnabled ? tint : Color(UIColor.systemGray4), in: Circle())
            .opacity(configuration.isPressed ? 0.7 : 1)
            .frame(width: 44, height: 44)
            .contentShape(Rectangle())
    }
}
