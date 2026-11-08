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
import dev.remoteagent.core.ErrorNotice
import dev.remoteagent.core.ErrorText
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.TurnErrorPresentation

private const val ERROR_SUMMARY_LINES = 3
private const val ERROR_SUMMARY_LENGTH = 180
private const val WARNING_COLOR = 0xFFE5B567

/** Error copy in the UI language above its verbatim cause. */
@Composable
internal fun ErrorTextView(
    text: ErrorText,
    modifier: Modifier = Modifier,
    color: androidx.compose.ui.graphics.Color = MaterialTheme.colorScheme.error,
) {
    Column(modifier) {
        Text(text.message, color = color)
        text.details?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** The copy, then the verbatim cause on its own line, for single-string surfaces. */
internal val ErrorText.plainText: String
    get() = listOfNotNull(message, details).joinToString("\n")

@Composable
internal fun ErrorNotices(
    notices: List<ErrorNotice>,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    reconnect: (() -> Unit)? = null,
) {
    val language = LocalUiLanguage.current
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        notices.forEach { notice ->
            var expanded by remember(notice) { mutableStateOf(false) }
            val details = notice.text.details.orEmpty()
            val hasDetails = details.length > ERROR_SUMMARY_LENGTH || details.lines().size > ERROR_SUMMARY_LINES
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
                        Column {
                            Text(
                                notice.text.message,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            if (details.isNotEmpty()) {
                                Text(
                                    details,
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = if (expanded || !hasDetails) Int.MAX_VALUE else ERROR_SUMMARY_LINES,
                                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                    if (hasDetails) {
                        TextButton(onClick = { expanded = !expanded }) {
                            Text(if (expanded) language.text("詳細を閉じる") else language.text("詳細を表示"))
                        }
                    }
                    notice.recoveryLabel?.let { label ->
                        TextButton(onClick = { perform(Intent.RecoverError(notice.id)) {} }) { Text(label) }
                    }
                    if (reconnect != null) {
                        TextButton(onClick = reconnect) { Text(language.text("再接続")) }
                    }
                    ErrorNoticeButtons(notice.title, notice.text.plainText) {
                        perform(Intent.DismissError(notice.id)) {}
                    }
                }
            }
        }
    }
}

@Composable
internal fun TurnErrorCard(error: TurnErrorPresentation) {
    val language = LocalUiLanguage.current
    var detailsExpanded by remember(error) { mutableStateOf(false) }
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainer) {
        Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                language.text("この応答"),
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
                        Text(if (detailsExpanded) language.text("詳細を閉じる") else language.text("詳細を表示"))
                    }
                    if (detailsExpanded) Text(details, style = MaterialTheme.typography.bodySmall)
                }
        }
    }
}

@Composable
private fun ErrorNoticeButtons(title: String, message: String, dismiss: () -> Unit) {
    val language = LocalUiLanguage.current
    val clipboard = LocalContext.current.getSystemService(ClipboardManager::class.java)
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        TextButton(
            onClick = {
                clipboard.setPrimaryClip(ClipData.newPlainText(language.text("エラーの詳細"), "${title}\n${message}"))
            }
        ) {
            Text(language.text("詳細をコピー"))
        }
        TextButton(onClick = { dismiss() }) { Text(language.text("閉じる")) }
    }
}
