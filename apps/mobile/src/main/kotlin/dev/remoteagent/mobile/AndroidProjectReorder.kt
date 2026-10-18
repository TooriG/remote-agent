package dev.remoteagent.mobile

import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LocalPinnableContainer
import androidx.compose.ui.layout.PinnableContainer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Intent
import dev.remoteagent.core.ProjectSummary
import kotlin.math.abs

internal data class ProjectDrop(val id: String, val after: Boolean)

// Only project headers are destinations; expanded chats and paging rows are skipped.
internal fun projectDropAt(y: Float, headers: List<Pair<String, IntRange>>): ProjectDrop? {
    val header = headers.minByOrNull { (_, bounds) ->
        when {
            y < bounds.first -> bounds.first - y
            y > bounds.last -> y - bounds.last
            else -> 0f
        }
    } ?: return null
    return ProjectDrop(header.first, y > (header.second.first + header.second.last) / 2f)
}

internal class ProjectReorder(val list: LazyListState) {
    var source by mutableStateOf<String?>(null)
    var target by mutableStateOf<ProjectDrop?>(null)
    var pointerY by mutableStateOf(0f)
    var projects: List<ProjectSummary> = emptyList()
    var perform: (Intent) -> Unit = {}
    var pinned: PinnableContainer.PinnedHandle? = null

    fun updateTarget() {
        target = projectDropAt(pointerY, list.layoutInfo.visibleItemsInfo.mapNotNull { item ->
            val key = item.key as? String ?: return@mapNotNull null
            val id = key.removePrefix("project:")
            if (projects.none { it.id == id }) return@mapNotNull null
            id to (item.offset until item.offset + item.size)
        })
    }

    fun finish(commit: Boolean) {
        val id = source
        val drop = target
        if (commit && id != null && drop != null && id != drop.id &&
            projects.any { it.id == id } && projects.any { it.id == drop.id }) {
            perform(Intent.ReorderProject(id, drop.id, drop.after))
        }
        source = null
        target = null
        pinned?.release()
        pinned = null
    }
}

@Composable
internal fun rememberProjectReorder(
    list: LazyListState,
    projects: List<ProjectSummary>,
    perform: (Intent) -> Unit,
): ProjectReorder {
    val state = remember(list) { ProjectReorder(list) }
    state.projects = projects
    state.perform = perform
    DisposableEffect(state) { onDispose { state.finish(commit = false) } }
    val edge = with(LocalDensity.current) { 56.dp.toPx() }
    LaunchedEffect(state.source) {
        var previous = withFrameNanos { it }
        while (state.source != null) {
            val now = withFrameNanos { it }
            val info = list.layoutInfo
            val velocity = when {
                state.pointerY < info.viewportStartOffset + edge -> -edge
                state.pointerY > info.viewportEndOffset - edge -> edge
                else -> 0f
            }
            val seconds = ((now - previous) / 1_000_000_000f).coerceAtMost(0.05f)
            if (abs(velocity) > 0f) list.scrollBy(velocity * seconds * 8f)
            state.updateTarget()
            previous = now
        }
    }
    return state
}

@Composable
internal fun ProjectDropMarker(state: ProjectReorder, id: String, after: Boolean) {
    // Reserve the same height throughout the gesture so headers never shift under the finger.
    Box(Modifier.height(2.dp)) {
        if (state.source != id && state.target == ProjectDrop(id, after)) {
            HorizontalDivider(thickness = 2.dp, color = MaterialTheme.colorScheme.primary)
        }
    }
}

@Composable
internal fun ProjectDragHandle(project: ProjectSummary, state: ProjectReorder) {
    val currentProject by rememberUpdatedState(project)
    val pinnable by rememberUpdatedState(LocalPinnableContainer.current)
    Box(
        Modifier.size(40.dp, 48.dp).semantics {
            contentDescription = "${project.name}の順番を変更。上下にドラッグ"
            val index = state.projects.indexOfFirst { it.id == project.id }
            customActions = listOfNotNull(
                state.projects.getOrNull(index - 1)?.let { previous ->
                    CustomAccessibilityAction("上へ移動") {
                        state.perform(Intent.ReorderProject(project.id, previous.id, false)); true
                    }
                },
                state.projects.getOrNull(index + 1)?.let { next ->
                    CustomAccessibilityAction("下へ移動") {
                        state.perform(Intent.ReorderProject(project.id, next.id, true)); true
                    }
                },
            )
        }.pointerInput(project.id, state) {
            detectDragGestures(
                onDragStart = { position ->
                    val row = state.list.layoutInfo.visibleItemsInfo
                        .firstOrNull { it.key == "project:${currentProject.id}" }
                    if (row != null) {
                        // Keep receiving this gesture when auto-scroll moves its source offscreen.
                        state.pinned = pinnable?.pin()
                        state.source = currentProject.id
                        state.pointerY = row.offset + row.size / 2f + position.y - size.height / 2f
                        state.updateTarget()
                    }
                },
                onDrag = { change, amount ->
                    change.consume()
                    state.pointerY += amount.y
                    state.updateTarget()
                },
                onDragEnd = { state.finish(commit = true) },
                onDragCancel = { state.finish(commit = false) },
            )
        },
        contentAlignment = Alignment.Center,
    ) {
        Text("≡", color = if (state.source == project.id) MaterialTheme.colorScheme.primary
            else MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
