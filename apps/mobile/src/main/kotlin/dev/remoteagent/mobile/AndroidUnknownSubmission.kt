package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome

@Composable
internal fun UnknownSubmissionActions(
    submissionId: String,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
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
                Text("下書きに戻す")
            }
            TextButton(enabled = !busy, onClick = { dispatch(Intent.DiscardUnknownSubmission(submissionId)) }) {
                Text("表示を削除")
            }
        }
        if (failed) Text("操作できませんでした。もう一度お試しください。", style = MaterialTheme.typography.bodySmall)
    }
}
