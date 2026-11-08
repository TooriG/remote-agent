package dev.remoteagent.mobile

import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import dev.remoteagent.core.shouldLoadHistory

@Composable
internal fun ObserveFollowing(
    listState: androidx.compose.foundation.lazy.LazyListState,
    projection: dev.remoteagent.core.ConversationRows?,
    following: Boolean,
    follow: (Boolean) -> Unit,
) {
    var previous by remember { mutableStateOf<dev.remoteagent.core.RenderedConversation?>(null) }
    LaunchedEffect(projection) {
        val source = projection?.source()
        val before = previous
        if (source != null && before != null && source.unchanged(before)) return@LaunchedEffect
        if (source != null && following) {
            withFrameNanos {}
            listState.scrollToLatest()
        }
        previous = source
    }
    LaunchedEffect(listState) {
        snapshotFlow { listState.isScrollInProgress to !listState.canScrollForward }
            .collect { (scrolling, bottom) -> if (scrolling) follow(bottom) }
    }
}

@Composable
internal fun ObserveHistoryPrefetch(
    listState: androidx.compose.foundation.lazy.LazyListState,
    threadId: dev.remoteagent.core.SessionRef?,
    history: dev.remoteagent.core.HistoryControl?,
    following: Boolean,
    perform: (dev.remoteagent.core.Intent, (Result<dev.remoteagent.core.Outcome>) -> Unit) -> Unit,
) {
    LaunchedEffect(threadId, history, following, perform) {
        val hasMore = history?.hasMore == true
        snapshotFlow {
            val viewportHeight = listState.layoutInfo.viewportEndOffset - listState.layoutInfo.viewportStartOffset
            shouldLoadHistory(
                hasMore,
                threadId == null || history?.loading == true || history?.error != null,
                viewportHeight > 0 &&
                    listState.firstVisibleItemIndex == 0 &&
                    listState.firstVisibleItemScrollOffset < viewportHeight * HISTORY_PREFETCH_FRACTION,
                !listState.canScrollForward,
                following,
            )
        }
            .collect { needed ->
                if (needed && threadId != null) perform(dev.remoteagent.core.Intent.ReadOlder(threadId)) {}
            }
    }
}

internal suspend fun androidx.compose.foundation.lazy.LazyListState.scrollToLatest() {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    scrollToItem(last)
    scroll { scrollBy(layoutInfo.visibleItemsInfo.lastOrNull()?.size?.toFloat() ?: 0f) }
}

private const val HISTORY_PREFETCH_FRACTION = 0.6
