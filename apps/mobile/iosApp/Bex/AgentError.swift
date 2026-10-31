import AgentCore
import Foundation

extension AgentError: @retroactive LocalizedError {
    public var errorDescription: String? {
        switch self {
        case let .Failed(reason): reason
        }
    }
}
