import AgentCore
import SwiftUI

struct ThreadRequestRow: View {
    let request: Request
    let respond: (Answer, @escaping (String?) -> Void) -> Void
    @State private var response = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            requestForm
            if busy {
                ProgressView()
            }
            if let error {
                AppText(error).foregroundColor(.red)
            }
        }
        .onAppear { response = requestInputDefault(body: request.requestBody) }
        .disabled(busy || !request.canRespond)
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.orange.opacity(0.14))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    private var requestForm: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(request.title).font(.subheadline.weight(.semibold))
                .accessibilityIdentifier("request.\(request.id)")
            Text(request.body).textSelection(.enabled)
            if !request.details.isEmpty {
                DisclosureGroup("詳細") { Text(request.details).font(.caption.monospaced()).textSelection(.enabled) }
            }
            switch request.requestBody {
            case let .approval(_, _, _, choices): choiceButtons(choices, permission: false)
            case let .permission(_, _, choices): choiceButtons(choices, permission: true)
            // The composer answers questions.
            case .question: EmptyView()
            case let .elicitation(_, _, input):
                if case let .url(url) = input {
                    if let url = URL(string: url) {
                        Link("リンクを開く", destination: url)
                    }
                } else {
                    if case let .form(fields) = input {
                        ForEach(fields, id: \.name) { field in
                            Text("\(field.title) (\(field.name))\(field.required ? " *" : "")")
                            if !field.description.isEmpty {
                                Text(field.description).font(.caption)
                            }
                        }
                    }
                    responseEditor
                }
                Button("確認して送信") { submitResponse() }
                HStack {
                    Button("辞退") { submit(.elicitation(action: .decline)) }
                    Button("キャンセル") { submit(.elicitation(action: .cancel)) }
                }
            case .toolExecution:
                responseEditor
                Button("実行結果を送信") { submitResponse() }
            }
        }
    }

    private var responseEditor: some View {
        TextEditor(text: $response).font(.body.monospaced()).frame(minHeight: 100)
    }

    private func choiceButtons(_ choices: [Choice], permission: Bool) -> some View {
        ForEach(choices, id: \.id) { choice in
            Button(choice.label) {
                submit(permission ? .permission(choiceId: choice.id) : .approval(choiceId: choice.id))
            }
            .accessibilityIdentifier(choice.id == choices.first?.id ? "request.accept" : "request.choice.\(choice.id)")
            if !choice.description.isEmpty {
                Text(choice.description).font(.caption)
            }
        }
    }

    private func submitResponse() {
        do {
            try submit(requestAnswerFromJson(body: request.requestBody, text: response))
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func submit(_ answer: Answer) {
        guard !busy, request.canRespond else { return }
        busy = true
        error = nil
        respond(answer) { message in busy = false; error = message }
    }
}

/// The question the composer answers, docked above it as in T3 Code.
struct QuestionPanel: View {
    let prompt: QuestionPrompt
    let busy: Bool
    let error: String?
    let choose: (String) -> Void
    let skip: () -> Void
    /// Collapsing applies to one question; the next question opens again.
    @State private var collapsedQuestionId: String?
    @State private var choicesHeight: CGFloat = 0

    var body: some View {
        let question = prompt.question
        let expanded = collapsedQuestionId != question.id
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Label(LocalizedStringKey(prompt.status ?? "質問"), systemImage: "questionmark.bubble")
                    .font(.subheadline).foregroundStyle(.secondary)
                    .accessibilityIdentifier("request.\(prompt.requestId)")
                if !question.header.isEmpty {
                    Text(question.header).font(.subheadline.weight(.semibold)).lineLimit(1)
                }
                Spacer(minLength: 0)
                if let progress = prompt.progress {
                    Text(progress).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }
                if busy {
                    ProgressView()
                }
                Button {
                    collapsedQuestionId = expanded ? question.id : nil
                } label: {
                    Image(systemName: expanded ? "chevron.down" : "chevron.up")
                        .font(.body.weight(.medium))
                        .foregroundStyle(.secondary)
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel(LocalizedStringKey(expanded ? "質問を閉じる" : "質問を開く"))
                .accessibilityIdentifier("request.toggle")
            }
            if expanded {
                Text(question.prompt).textSelection(.enabled)
                if question.multiple {
                    Text("複数選択できます").font(.caption).foregroundStyle(.secondary)
                }
                // Natural height, scrolling only when the options exceed the limit.
                ScrollView {
                    choices(question)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { choicesHeight = $0 }
                }
                .scrollBounceBehavior(.basedOnSize)
                .frame(height: min(choicesHeight, 240))
                HStack {
                    Spacer(minLength: 0)
                    Button("スキップする", action: skip)
                        .font(.body.weight(.semibold))
                        .padding(.horizontal, 20).padding(.vertical, 12)
                        .overlay(Capsule().strokeBorder(Color.primary.opacity(0.15), lineWidth: 1))
                        .accessibilityIdentifier("request.skip")
                }
            } else {
                Text(question.prompt).lineLimit(1).foregroundStyle(.secondary)
            }
            if let error {
                AppText(error).font(.caption).foregroundColor(.red)
            }
        }
        .disabled(busy || !prompt.canRespond)
        .buttonStyle(.plain)
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(uiColor: .secondarySystemGroupedBackground))
        .clipShape(RoundedRectangle(cornerRadius: 24))
        .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(Color.primary.opacity(0.15), lineWidth: 1))
    }

    private func choices(_ question: Question) -> some View {
        VStack(spacing: 0) {
            ForEach(question.choices, id: \.id) { choice in
                let chosen = prompt.chosenChoiceIds.contains(choice.id)
                Button { choose(choice.id) } label: {
                    choiceLabel(label: choice.label, description: choice.description, chosen: chosen)
                }
                .accessibilityLabel(choice.label)
                .accessibilityHint(choice.description)
                .accessibilityValue(chosen ? "選択済み" : "未選択")
                .accessibilityAddTraits(chosen ? [.isSelected] : [])
                .accessibilityIdentifier("request.choice.\(choice.id)")
                if choice.id != question.choices.last?.id {
                    Divider()
                }
            }
        }
    }

    private func choiceLabel(label: String, description: String, chosen: Bool) -> some View {
        HStack(alignment: .center, spacing: 12) {
            ZStack {
                Circle().strokeBorder(Color.primary.opacity(0.2), lineWidth: 2)
                if chosen {
                    Circle().fill(Color.primary)
                    Image(systemName: "checkmark")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color(uiColor: .systemBackground))
                }
            }
            .frame(width: 24, height: 24)
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(label).font(.body)
                if !description.isEmpty {
                    Text(description).font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// Composer controls while a question awaits an answer.
extension ThreadScreen {
    /// While a question awaits an answer, the primary action answers it.
    @ViewBuilder
    func questionActions(_ question: QuestionPrompt, runningTurnId: String?) -> some View {
        if let runningTurnId {
            stopButton(runningTurnId)
        }
        if question.canGoBack {
            Button("前へ") { model.answerQuestion(question.requestId, .back) }
                .font(.subheadline.weight(.semibold))
                .padding(.horizontal, 12)
                .frame(minHeight: 44)
                .accessibilityIdentifier("request.back")
        }
        let enabled = question.canAdvance && model.isConnected
            && !model.snapshot.operationRunning(key: .request(id: question.requestId))
        Button {
            // Commit native text before the answer is read.
            UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
            composerFocused = false
            model.answerQuestion(question.requestId, .advance)
        } label: {
            AppText(question.action)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(enabled ? Color.white : Color.primary)
                .padding(.horizontal, 16)
                .frame(height: 32)
                .background(enabled ? Color.accentColor : Color(UIColor.systemGray4), in: Capsule())
                .frame(minHeight: 44)
                .contentShape(Rectangle())
        }
        .disabled(!enabled)
        .accessibilityIdentifier("request.send")
    }

    /// The composer text is the active question's own answer.
    func answerField(_ question: QuestionPrompt) -> some View {
        // Read the published answer, not this render's copy, as the draft field does.
        BufferedTextInput(value: Binding(get: { model.questionPrompt?.text ?? "" }, set: {
            model.answerQuestion(question.requestId, .edit(text: $0))
        })) { input in
            Group {
                if question.question.secret {
                    SecureField(question.placeholder, text: input)
                } else {
                    TextField(question.placeholder, text: input, axis: .vertical)
                        .lineLimit(1 ... 6)
                }
            }
            .focused($composerFocused)
            .disabled(!question.question.allowFreeText || !question.canRespond)
            .accessibilityIdentifier("task.message")
        }
        .id("\(question.requestId)/\(question.question.id)")
    }
}
