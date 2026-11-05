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
import dev.remoteagent.core.localizedUiFormat
import kotlin.math.abs

private const val NANOS_PER_SECOND = 1_000_000_000f
private const val MAX_FRAME_SECONDS = 0.05f
private const val SCROLL_SPEED = 8f

internal data class ProjectDrop(val id: String, val after: Boolean)

// Only project headers are destinations; expanded chats and paging rows are skipped.
internal fun projectDropAt(y: Float, headers: List<Pair<String, IntRange>>): ProjectDrop? {
    val header =
        headers.minByOrNull { (_, bounds) ->
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
    var grabOffsetY by mutableStateOf(0f)
    val draggedTop: Float
        get() = pointerY - grabOffsetY

    var projects: List<ProjectSummary> = emptyList()
    var perform: (Intent, String) -> Unit = { _, _ -> }
    var pinned: PinnableContainer.PinnedHandle? = null

    fun moveActions(
        id: String,
        host: String,
        language: dev.remoteagent.core.UiLanguage,
    ): List<CustomAccessibilityAction> {
        val index = projects.indexOfFirst { it.id == id && it.hostId == host }
        if (index < 0) return emptyList()
        return listOf(index - 1 to false, index + 1 to true).mapNotNull { (targetIndex, after) ->
            projects
                .getOrNull(targetIndex)
                ?.takeIf { it.hostId == host }
                ?.let { target ->
                    CustomAccessibilityAction(language.text(if (after) "下へ移動" else "上へ移動")) {
                        perform(Intent.ReorderProject(id, target.id, after), host)
                        true
                    }
                }
        }
    }

    fun updateTarget() {
        val host = projects.firstOrNull { it.listKey == source }?.hostId
        target =
            projectDropAt(
                pointerY,
                list.layoutInfo.visibleItemsInfo.mapNotNull { item ->
                    val key = item.key as? String ?: return@mapNotNull null
                    val id = key.removePrefix("project:")
                    if (projects.none { it.listKey == id && it.hostId == host }) return@mapNotNull null
                    id to (item.offset until item.offset + item.size)
                },
            )
    }

    fun finish(commit: Boolean) {
        val id = source
        val drop = target
        if (commit && id != null && drop != null) {
            val from = projects.firstOrNull { it.listKey == id }
            val to = projects.firstOrNull { it.listKey == drop.id }
            if (from != null && to != null) {
                if (from.listKey != to.listKey && from.hostId == to.hostId) {
                    perform(Intent.ReorderProject(from.id, to.id, drop.after), from.hostId)
                }
            }
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
    perform: (Intent, String) -> Unit,
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
            val velocity =
                when {
                    state.pointerY < info.viewportStartOffset + edge -> -edge
                    state.pointerY > info.viewportEndOffset - edge -> edge
                    else -> 0f
                }
            val seconds = ((now - previous) / NANOS_PER_SECOND).coerceAtMost(MAX_FRAME_SECONDS)
            if (abs(velocity) > 0f) list.scrollBy(velocity * seconds * SCROLL_SPEED)
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
    val language = LocalUiLanguage.current
    val currentProject by rememberUpdatedState(project)
    val pinnable by rememberUpdatedState(LocalPinnableContainer.current)
    Box(
        Modifier.size(40.dp, 48.dp)
            .semantics {
                contentDescription = localizedUiFormat(language, "{0}の順番を変更。上下にドラッグ", listOf(project.name))
                customActions = state.moveActions(project.id, project.hostId, language)
            }
            .pointerInput(project.listKey, state) {
                detectDragGestures(
                    onDragStart = { position ->
                        val row =
                            state.list.layoutInfo.visibleItemsInfo.firstOrNull {
                                it.key == "project:${currentProject.listKey}"
                            }
                        if (row != null) {
                            // Keep receiving this gesture when auto-scroll moves its source offscreen.
                            state.pinned = pinnable?.pin()
                            state.source = currentProject.listKey
                            state.pointerY = row.offset + row.size / 2f + position.y - size.height / 2f
                            state.grabOffsetY = state.pointerY - row.offset
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
        Text(
            "≡",
            color =
                if (state.source == project.listKey) MaterialTheme.colorScheme.primary
                else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

internal val ProjectSummary.listKey: String
    get() = "$hostId:$id"
