import AgentCore
import SwiftUI

struct ThreadRequestRow: View {
    let request: Request
    let respond: (Answer, @escaping (String?) -> Void) -> Void
    @State private var answers: [String: QuestionAnswer] = [:]
    @State private var response = ""
    @State private var busy = false
    @State private var error: String?
    @State private var questionExpanded = true

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if case let .question(questions) = request.requestBody {
                questionCard(questions)
            } else {
                requestForm
            }
            if busy {
                ProgressView()
            }
            if let error {
                Text(error).foregroundColor(.red)
            }
        }
        .onAppear { response = requestInputDefault(body: request.requestBody) }
        .disabled(busy || !request.canRespond)
        .padding(isQuestion ? 16 : 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isQuestion ? Color(uiColor: .secondarySystemGroupedBackground) : Color.orange.opacity(0.14))
        .clipShape(RoundedRectangle(cornerRadius: isQuestion ? 24 : 10))
        .overlay {
            if isQuestion {
                RoundedRectangle(cornerRadius: 24).strokeBorder(Color.primary.opacity(0.15), lineWidth: 1)
            }
        }
    }

    private var isQuestion: Bool {
        if case .question = request.requestBody {
            return true
        }
        return false
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

    private func questionCard(_ questions: [Question]) -> some View {
        VStack(alignment: .leading, spacing: 24) {
            HStack {
                Label(request.canRespond ? "質問" : request.title, systemImage: "questionmark.bubble")
                    .font(.subheadline).foregroundStyle(.secondary)
                    .accessibilityIdentifier("request.\(request.id)")
                Spacer()
                Button {
                    questionExpanded.toggle()
                } label: {
                    Image(systemName: questionExpanded ? "xmark" : "chevron.down")
                        .font(.body.weight(.medium))
                        .foregroundStyle(.secondary)
                        .frame(width: 44, height: 44)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel(questionExpanded ? "質問を閉じる" : "質問を開く")
                .accessibilityIdentifier("request.toggle")
            }
            if questionExpanded {
                ForEach(questions, id: \.id) { question in
                    VStack(alignment: .leading, spacing: 20) { questionView(question) }
                }
                HStack(spacing: 8) {
                    Spacer(minLength: 0)
                    Button("スキップする") { submit(.skipQuestions) }
                        .padding(.horizontal, 20).padding(.vertical, 12)
                        .overlay(Capsule().strokeBorder(Color.primary.opacity(0.15), lineWidth: 1))
                        .accessibilityIdentifier("request.skip")
                    let answer = Answer.questions(answers: answers)
                    let canSend = requestAnswerIsValid(body: request.requestBody, answer: answer)
                    Button("送信") { submit(answer) }
                        .padding(.horizontal, 20).padding(.vertical, 12)
                        .foregroundStyle(Color(uiColor: .systemBackground))
                        .background(Color.primary, in: Capsule())
                        .disabled(!canSend)
                        .opacity(canSend ? 1 : 0.35)
                        .accessibilityIdentifier("request.send")
                }
                .font(.body.weight(.semibold))
            }
        }
        .buttonStyle(.plain)
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

    @ViewBuilder private func questionView(_ question: Question) -> some View {
        let id = question.id
        if !question.header.isEmpty {
            Text(question.header).font(.subheadline.weight(.semibold))
        }
        Text(question.prompt).textSelection(.enabled)
        VStack(spacing: 0) {
            ForEach(question.choices, id: \.id) { choice in
                let selected = selectedChoiceIds(answers[id]).contains(choice.id)
                Button {
                    selectChoice(choice.id, questionId: id, multiple: question.multiple)
                }
                label: {
                    questionChoiceLabel(label: choice.label, description: choice.description, selected: selected)
                }
                .accessibilityLabel(choice.label)
                .accessibilityHint(choice.description)
                .accessibilityValue(selected ? "選択済み" : "未選択")
                .accessibilityAddTraits(selected ? [.isSelected] : [])
                .accessibilityIdentifier("request.choice.\(choice.id)")
                if choice.id != question.choices.last?.id {
                    Divider()
                }
            }
        }
        if question.allowFreeText {
            questionReply(id: id, secret: question.secret)
        }
    }

    private func selectChoice(_ choiceId: String, questionId: String, multiple: Bool) {
        if multiple {
            var selected = selectedChoiceIds(answers[questionId])
            if selected.contains(choiceId) {
                selected.removeAll { $0 == choiceId }
            } else {
                selected.append(choiceId)
            }
            answers[questionId] = .multipleChoices(choiceIds: selected)
        } else {
            answers[questionId] = .singleChoice(choiceId: choiceId)
            let answer = Answer.questions(answers: answers)
            if requestAnswerIsValid(body: request.requestBody, answer: answer) {
                submit(answer)
            }
        }
    }

    private func selectedChoiceIds(_ answer: QuestionAnswer?) -> [String] {
        switch answer {
        case let .singleChoice(id): [id]
        case let .multipleChoices(ids): ids
        default: []
        }
    }

    private func questionReply(id: String, secret: Bool) -> some View {
        let binding = Binding<String>(get: {
            if case let .freeText(text) = answers[id] {
                return text
            }
            return ""
        }, set: { answers[id] = .freeText(text: $0) })
        return Group {
            if secret {
                SecureField("返信…", text: binding)
            } else {
                TextField("返信…", text: binding)
            }
        }
        .padding(12)
        .frame(minHeight: 48)
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.primary.opacity(0.15), lineWidth: 1))
        .accessibilityLabel("回答")
        .accessibilityIdentifier("request.answer")
    }

    private func questionChoiceLabel(label: String, description: String, selected: Bool) -> some View {
        HStack(alignment: .center, spacing: 12) {
            ZStack {
                Circle().strokeBorder(Color.primary.opacity(0.2), lineWidth: 2)
                if selected {
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
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
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
