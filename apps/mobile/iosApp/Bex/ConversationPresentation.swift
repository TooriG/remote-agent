import AgentCore

struct ThreadConversationRow: Identifiable, Sendable {
    let id: String
    let content: Content
    enum Content: Sendable {
        case historyNotice(String)
        case native(VisibleConversationRow, ConversationItem?)
    }
}

/// Immutable native render values; Rust owns projection policy.
struct ConversationPresentation: Sendable {
    let source: AgentCore.Thread
    let id: SessionRef?
    let scope: PanelScope
    private struct CachedTurn: Sendable {
        let source: RenderedTurn
        let items: [String: ConversationItem]
    }

    private let turns: [String: CachedTurn]
    let projection: RenderedConversation
    private let queued: [String: ConversationItem]

    static func project(_ rendered: RenderedConversation, scope: PanelScope, previous: Self?) -> Self {
        let source = rendered.source()
        if let previous, previous.scope == scope, rendered.unchanged(other: previous.projection) {
            return previous
        }
        var next: [String: CachedTurn] = [:]
        for turn in rendered.turns() {
            let id = turn.key()
            if let cached = previous?.turns[id], turn.unchanged(other: cached.source) {
                next[id] = cached
                continue
            }
            let cached = previous?.turns[id]?.items ?? [:]
            var items: [String: ConversationItem] = [:]
            for row in turn.conversationRows() {
                switch row.content {
                case let .user(source), let .response(source, _, _), let .activity(source, _):
                    items[row.id] = Self.item(source, previous: cached[row.id])
                default: break
                }
            }
            next[id] = CachedTurn(source: turn, items: items)
        }
        var nextQueued: [String: ConversationItem] = [:]
        for item in rendered.queued() {
            nextQueued[item.id()] = Self.item(item, previous: previous?.queued[item.id()])
        }
        return Self(
            source: source,
            id: source.id(),
            scope: scope, turns: next, projection: rendered, queued: nextQueued
        )
    }

    func rows(_ projected: [VisibleConversationRow]) -> [ThreadConversationRow] {
        let notice = source.historyNotice().map {
            [ThreadConversationRow(id: "history-read-state", content: .historyNotice($0))]
        } ?? []
        return notice + projected.map { row in
            ThreadConversationRow(id: row.id(), content: .native(row, item(for: row)))
        }
    }

    private func item(for row: VisibleConversationRow) -> ConversationItem? {
        switch row.origin() {
        case let .turn(key, _, _, _): turns[key]?.items[row.sourceId()]
        case .queued: queued[row.sourceId()]
        case .session: nil
        }
    }

    private static func item(_ source: RenderedItem, previous: ConversationItem?) -> ConversationItem {
        if let previous, source.unchanged(other: previous.source) {
            return previous
        }
        return ConversationItem(source)
    }
}
