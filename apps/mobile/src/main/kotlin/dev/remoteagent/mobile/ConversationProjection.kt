package dev.remoteagent.mobile

import dev.remoteagent.core.ConversationRow
import dev.remoteagent.core.RenderedConversation
import dev.remoteagent.core.RenderedItem
import dev.remoteagent.core.RenderedTurn

/** Previous values are inputs; projecting rows never mutates a published result. */
internal data class ConversationProjection(
    val source: RenderedConversation,
    val turns: Map<String, Pair<RenderedTurn, List<ConversationRow>>>,
    val rows: List<ConversationRow>,
    val queued: List<RenderedItem>,
    val sessionRows: List<ConversationRow>,
)

internal fun projectConversationRows(
    next: RenderedConversation?,
    previous: ConversationProjection?,
): ConversationProjection? {
    if (next == null) return null
    return if (previous?.source?.let(next::unchanged) == true) previous
    else {
        val turns = mutableMapOf<String, Pair<RenderedTurn, List<ConversationRow>>>()
        val rows =
            next.turns().flatMap { turn ->
                val old = previous?.turns?.get(turn.id())
                val content = if (old != null && turn.unchanged(old.first)) old.second else turn.conversationRows()
                turns[turn.id()] = turn to content
                content
            }
        ConversationProjection(next, turns, rows, next.queued(), next.sessionRows())
    }
}
