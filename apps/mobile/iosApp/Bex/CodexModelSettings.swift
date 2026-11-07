import AgentCore
import Foundation

extension ConversationModel {
    var accounts: [Account] {
        settings.accounts()?.accounts ?? []
    }

    var selectedModel: ModelRef? {
        settings.selectedModel()
    }

    func chooseModel(_ value: ModelRef) {
        perform(.selectModel(threadId: coreDraftKey, model: value))
    }

    func chooseEffort(_ value: String) {
        perform(.selectEffort(threadId: coreDraftKey, effort: value))
    }

    func chooseServiceTier(_ value: String) {
        perform(.selectServiceTier(threadId: coreDraftKey, serviceTier: value))
    }
}
