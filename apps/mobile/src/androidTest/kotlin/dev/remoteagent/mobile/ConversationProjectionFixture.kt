package dev.remoteagent.mobile

import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.Thread
import dev.remoteagent.core.projectConversation

/** Construct native action fixtures from immutable core render values. */
internal fun projectConversationRows(
    snapshot: Snapshot,
    thread: Thread?,
    previous: ConversationProjection?,
): ConversationProjection? =
    projectConversationRows(thread?.let { projectConversation(snapshot, it, previous?.source) }, previous)

@androidx.compose.runtime.Composable
internal fun ThreadDetailFixture(
    snapshot: Snapshot,
    projection: ConversationProjection?,
    perform: (dev.remoteagent.core.Intent, (Result<dev.remoteagent.core.Outcome>) -> Unit) -> Unit,
    older: (() -> Unit)?,
    modifier: androidx.compose.ui.Modifier = androidx.compose.ui.Modifier,
    scrollToTopRequest: Int = 0,
    composer: @androidx.compose.runtime.Composable (() -> Unit) -> Unit,
) {
    ThreadDetailScreen(
        dev.remoteagent.core.ConversationContext.fromSnapshot(snapshot),
        projection,
        perform,
        older,
        modifier,
        scrollToTopRequest,
        composer,
    )
}
