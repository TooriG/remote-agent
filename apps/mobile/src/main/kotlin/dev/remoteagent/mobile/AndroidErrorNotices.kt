package dev.remoteagent.mobile

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.ErrorScope
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.TurnErrorPresentation

private const val ERROR_SUMMARY_LINES = 3
private const val ERROR_SUMMARY_LENGTH = 180
private const val WARNING_COLOR = 0xFFE5B567

@Composable
internal fun ErrorNotices(
    snapshot: Snapshot,
    scope: ErrorScope,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    reconnect: (() -> Unit)? = null,
) {
    val clipboard = LocalContext.current.getSystemService(ClipboardManager::class.java)
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        snapshot.errorNotices(scope).forEach { notice ->
            var expanded by remember(notice) { mutableStateOf(false) }
            val hasDetails =
                notice.message.length > ERROR_SUMMARY_LENGTH || notice.message.lines().size > ERROR_SUMMARY_LINES
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                color = MaterialTheme.colorScheme.surfaceContainer,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.error.copy(alpha = 0.2f)),
            ) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        notice.context,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(notice.title, style = MaterialTheme.typography.titleSmall)
                    SelectionContainer {
                        Text(
                            notice.message,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = if (expanded || !hasDetails) Int.MAX_VALUE else ERROR_SUMMARY_LINES,
                            overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                        )
                    }
                    if (hasDetails) {
                        TextButton(onClick = { expanded = !expanded }) { Text(if (expanded) "詳細を閉じる" else "詳細を表示") }
                    }
                    snapshot.errorRecovery(notice.id)?.let { recovery ->
                        TextButton(onClick = { perform(recovery.intent) {} }) { Text(recovery.label) }
                    }
                    if (scope == ErrorScope.Application && !snapshot.connected() && reconnect != null) {
                        TextButton(onClick = reconnect) { Text("再接続") }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(
                            onClick = {
                                clipboard.setPrimaryClip(
                                    ClipData.newPlainText("エラーの詳細", "${notice.title}\n${notice.message}")
                                )
                            }
                        ) {
                            Text("詳細をコピー")
                        }
                        TextButton(onClick = { perform(Intent.DismissError(notice.id)) {} }) { Text("閉じる") }
                    }
                }
            }
        }
    }
}

@Composable
internal fun TurnErrorCard(error: TurnErrorPresentation) {
    var detailsExpanded by remember(error) { mutableStateOf(false) }
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainer) {
        Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                "この応答",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (error.isReconnecting) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                Text(
                    error.title,
                    style = MaterialTheme.typography.titleSmall,
                    color =
                        if (error.isReconnecting) MaterialTheme.colorScheme.onSurface
                        else if (error.isUsageLimit) androidx.compose.ui.graphics.Color(WARNING_COLOR)
                        else MaterialTheme.colorScheme.error,
                )
            }
            androidx.compose.foundation.text.selection.SelectionContainer {
                Text(error.message, style = MaterialTheme.typography.bodySmall)
            }
            error.details
                ?.takeIf { it.isNotEmpty() }
                ?.let { details ->
                    TextButton(onClick = { detailsExpanded = !detailsExpanded }) {
                        Text(if (detailsExpanded) "詳細を閉じる" else "詳細を表示")
                    }
                    if (detailsExpanded) Text(details, style = MaterialTheme.typography.bodySmall)
                }
        }
    }
}
