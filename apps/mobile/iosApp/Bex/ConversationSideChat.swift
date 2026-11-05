import AgentCore
import Observation
import SwiftUI

@MainActor
@Observable
final class SideChatRequest {
    let id = UUID()
    let text: String
    let host: String
    let originalThreadId: SessionRef
    let cwd: String
    @ObservationIgnored private let session: ClientSession
    @ObservationIgnored private let provider: ProviderKind?
    private(set) var chat: ConversationModel?
    private(set) var preparing = false
    private(set) var error: ErrorText?
    private var draftInitialized = false

    init(text: String, host: String, originalThreadId: SessionRef, cwd: String,
         session: ClientSession, provider: ProviderKind?) {
        self.text = text
        self.host = host
        self.originalThreadId = originalThreadId
        self.cwd = cwd
        self.session = session
        self.provider = provider
    }

    func prepare() async {
        guard !preparing else { return }
        preparing = true
        error = nil
        defer { preparing = false }
        do {
            let model: ConversationModel
            if let chat {
                model = chat
            } else {
                let store = try await session.ready()
                let pane = try await store.openPane(cwd: cwd)
                model = ConversationModel(host: host, session: session, pane: pane)
                chat = model
            }
            if model.selectedThreadId == nil {
                guard let provider = provider ?? model.snapshot.modelProviderForDraft(threadId: model.coreDraftKey)
                else {
                    throw NSError(domain: "Bex", code: 1,
                                  userInfo: [NSLocalizedDescriptionKey: "利用可能なエージェントを選択してください。"])
                }
                _ = try await model.outcome(for: .createSession(CreateSession(
                    provider: provider, cwd: cwd, model: nil, initialInput: nil
                )))
            }
            if !draftInitialized {
                if !text.isEmpty {
                    model.draft = ConversationModel.selectionQuote(text)
                }
                draftInitialized = true
            }
            model.composerFocusRequest = UUID()
        } catch {
            if !Task.isCancelled {
                self.error = errorText(error: error.localizedDescription)
            }
        }
    }
}

extension ConversationModel {
    func addSelectionToChat(_ text: String) {
        let quote = Self.selectionQuote(text)
        draft = draft.isEmpty ? quote : draft + "\n\n" + quote
        composerFocusRequest = UUID()
    }

    static func selectionQuote(_ text: String) -> String {
        text.components(separatedBy: "\n").map { "> " + $0 }.joined(separator: "\n") + "\n\n"
    }

    func askSelectionInSideChat(_ text: String) {
        guard let thread = selectedThreadId else { return }
        sideChatRequest = SideChatRequest(text: text, host: host, originalThreadId: thread,
                                          cwd: selectedDirectory, session: session,
                                          provider: snapshot.modelProviderForDraft(threadId: coreDraftKey))
    }
}

extension ThreadScreen {
    var selectionActions: ConversationSelectionActions {
        let ask: ((String) -> Void)? = isSideChat ? nil : { text in
            model.askSelectionInSideChat(text)
            openTools?(.sideChat)
        }
        return ConversationSelectionActions(addToChat: model.addSelectionToChat, askInSideChat: ask)
    }
}

struct ConversationSideChat: View {
    let request: SideChatRequest
    let environment: ConversationEnvironment

    var body: some View {
        VStack(spacing: 0) {
            if request.preparing {
                ProgressView("サイドチャットを準備中…")
            } else if let error = request.error {
                VStack(spacing: 16) {
                    ErrorTextView(text: error).foregroundStyle(.red)
                    Button("再試行") { Task { await request.prepare() } }
                }.padding()
            } else if let chat = request.chat {
                ThreadScreen(model: chat, environment: environment,
                             conversation: chat.conversation, isSideChat: true)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task { await request.prepare() }
    }
}
