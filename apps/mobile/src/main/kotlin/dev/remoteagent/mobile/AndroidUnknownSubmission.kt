package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.RenderedItem
import dev.remoteagent.core.localizedUiFormat
import kotlin.coroutines.resume
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine

/** Recovery stays local: only core's explicit discard intent removes a retained input. */
@Composable
internal fun UnknownSubmissionSummary(
    submissionIds: List<String>,
    expanded: Boolean,
    setExpanded: (Boolean) -> Unit,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    var busy by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    Card {
        Column(Modifier.padding(12.dp)) {
            Text(
                localizedUiFormat(language, "送信結果不明・{0}件", listOf(submissionIds.size.toString())),
                style = MaterialTheme.typography.labelLarge,
            )
            Text(language.text("内容は保存されています。自動再送はしません。"), style = MaterialTheme.typography.bodySmall)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(enabled = !busy, onClick = { setExpanded(!expanded) }) {
                    Text(if (expanded) language.text("閉じる") else language.text("内容を確認"))
                }
                TextButton(
                    enabled = !busy,
                    onClick = {
                        // Freeze this batch so newly arriving inputs are never discarded.
                        val batch = submissionIds.toList()
                        busy = true
                        failed = false
                        setExpanded(false)
                        scope.launch {
                            try {
                                failed = !discardUnknownSubmissions(batch, perform)
                            } finally {
                                busy = false
                            }
                        }
                    },
                ) {
                    Text(if (busy) language.text("削除中…") else language.text("表示をすべて削除"))
                }
            }
            if (failed) Text(language.text("削除できませんでした。残った表示をもう一度削除できます。"), style = MaterialTheme.typography.bodySmall)
        }
    }
}

private suspend fun discardUnknownSubmissions(
    batch: List<String>,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
): Boolean {
    for (id in batch) {
        val result =
            suspendCancellableCoroutine<Result<Outcome>> { continuation ->
                perform(Intent.DiscardUnknownSubmission(id)) { result ->
                    if (continuation.isActive) continuation.resume(result)
                }
            }
        if (result.isFailure) return false
    }
    return true
}

@Composable
internal fun UnknownSubmissionActions(submissionId: String, perform: (Intent, (Result<Outcome>) -> Unit) -> Unit) {
    val language = LocalUiLanguage.current
    var busy by remember(submissionId) { mutableStateOf(false) }
    var failed by remember(submissionId) { mutableStateOf(false) }
    fun dispatch(intent: Intent) {
        busy = true
        failed = false
        perform(intent) { result ->
            busy = false
            failed = result.isFailure
        }
    }
    Column {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(enabled = !busy, onClick = { dispatch(Intent.RestoreUnknownSubmission(submissionId)) }) {
                Text(language.text("下書きに戻す"))
            }
            TextButton(enabled = !busy, onClick = { dispatch(Intent.DiscardUnknownSubmission(submissionId)) }) {
                Text(language.text("表示を削除"))
            }
        }
        if (failed) Text(language.text("操作できませんでした。もう一度お試しください。"), style = MaterialTheme.typography.bodySmall)
    }
}

internal fun LazyListScope.pendingInputs(
    queued: List<RenderedItem>,
    cwd: String,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    expanded: Boolean,
    setExpanded: (Boolean) -> Unit,
) {
    val unknownIds = queued.mapNotNull { it.unknownSubmissionId() }
    if (unknownIds.isNotEmpty()) {
        item(key = "unknown-submission-summary") {
            UnknownSubmissionSummary(unknownIds, expanded, setExpanded, perform)
        }
    }
    items(queued.filter { expanded || it.unknownSubmissionId() == null }, key = { "queued:${it.id()}" }) {
        ThreadMessageCard(it, true, cwd, perform)
    }
}
