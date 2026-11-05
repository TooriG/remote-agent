package dev.remoteagent.mobile

import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import dev.remoteagent.core.shouldLoadHistory

@Composable
internal fun ObserveFollowing(
    listState: androidx.compose.foundation.lazy.LazyListState,
    projection: ConversationProjection?,
    following: Boolean,
    follow: (Boolean) -> Unit,
) {
    LaunchedEffect(projection) {
        if (following) {
            withFrameNanos {}
            listState.scrollToLatest()
        }
    }
    LaunchedEffect(listState) {
        snapshotFlow { listState.isScrollInProgress to !listState.canScrollForward }
            .collect { (scrolling, bottom) -> if (scrolling) follow(bottom) }
    }
}

@Composable
internal fun ObserveHistoryPrefetch(
    listState: androidx.compose.foundation.lazy.LazyListState,
    history: dev.remoteagent.core.HistoryControl?,
    following: Boolean,
    older: (() -> Unit)?,
) {
    LaunchedEffect(history, following, older) {
        val hasMore = history?.hasMore == true
        snapshotFlow {
            val viewportHeight = listState.layoutInfo.viewportEndOffset - listState.layoutInfo.viewportStartOffset
            shouldLoadHistory(
                hasMore,
                older == null || history?.error != null,
                viewportHeight > 0 &&
                    listState.firstVisibleItemIndex == 0 &&
                    listState.firstVisibleItemScrollOffset < viewportHeight * HISTORY_PREFETCH_FRACTION,
                !listState.canScrollForward,
                following,
            )
        }
            .collect { needed -> if (needed) older?.invoke() }
    }
}

internal suspend fun androidx.compose.foundation.lazy.LazyListState.scrollToLatest() {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    scrollToItem(last)
    scroll { scrollBy(layoutInfo.visibleItemsInfo.lastOrNull()?.size?.toFloat() ?: 0f) }
}

private const val HISTORY_PREFETCH_FRACTION = 0.6
