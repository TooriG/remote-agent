package dev.remoteagent.mobile

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import dev.remoteagent.core.ConversationRowPolicy
import dev.remoteagent.core.ConversationRows
import dev.remoteagent.core.ProgressRows
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.Thread
import dev.remoteagent.core.projectConversation

private val fixtureRowPolicy = ConversationRowPolicy(requests = true, progress = ProgressRows.ALWAYS)

/** Construct native action fixtures through the production core projector. */
internal fun projectConversationRows(
    snapshot: Snapshot,
    thread: Thread?,
    previous: ConversationRows?,
): ConversationRows? = thread?.let {
    ConversationRows.project(
        projectConversation(snapshot, it, previous?.source()),
        snapshot.panelScope("fixture", snapshot.navigation().draftKey),
        emptyMap(),
        fixtureRowPolicy,
        previous,
    )
}

@androidx.compose.runtime.Composable
internal fun ThreadDetailFixture(
    snapshot: Snapshot,
    projection: ConversationRows?,
    perform: (dev.remoteagent.core.Intent, (Result<dev.remoteagent.core.Outcome>) -> Unit) -> Unit,
    modifier: androidx.compose.ui.Modifier = androidx.compose.ui.Modifier,
    scrollToTopRequest: Int = 0,
    composer: @androidx.compose.runtime.Composable (() -> Unit) -> Unit,
) {
    var current by remember(projection) { mutableStateOf(projection) }
    ThreadDetailScreen(
        dev.remoteagent.core.ConversationContext.fromSnapshot(snapshot),
        current,
        perform,
        { id ->
            current?.let { before ->
                before.toggleActivity(id)?.let { change ->
                    current =
                        ConversationRows.project(
                            before.source(),
                            before.scope(),
                            change.expansion,
                            fixtureRowPolicy,
                            before,
                        )
                    change.loadItems?.let { perform(dev.remoteagent.core.Intent.LoadTurnItems(it)) {} }
                }
            }
        },
        modifier,
        scrollToTopRequest,
        composer,
    )
}
