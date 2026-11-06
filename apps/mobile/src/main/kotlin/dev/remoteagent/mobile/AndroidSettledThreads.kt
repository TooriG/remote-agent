package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Intent
import dev.remoteagent.core.SetSessionSettled
import dev.remoteagent.core.SettledThreads
import dev.remoteagent.core.ThreadSummary

@Composable
internal fun ThreadSettlementMenu(thread: ThreadSummary, openConversation: (Intent) -> Unit) {
    var menuOpen by remember { mutableStateOf(false) }
    val language = LocalUiLanguage.current
    Box {
        TextButton(onClick = { menuOpen = true }) { Text("⋯") }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            DropdownMenuItem(
                text = { Text(language.text(if (thread.settled) "Active に戻す" else "片づける")) },
                enabled = thread.settled || thread.canSettle,
                onClick = {
                    menuOpen = false
                    openConversation(Intent.SetSessionSettled(SetSessionSettled(thread.id, !thread.settled)))
                },
            )
        }
    }
}

internal fun LazyListScope.settledSection(
    shelf: SettledThreads,
    perform: (Intent) -> Unit,
    language: dev.remoteagent.core.UiLanguage,
) {
    val key = "settled:${shelf.hostId}:${shelf.projectId}"
    item(key = key) {
        TextButton(onClick = { perform(Intent.SetSettledExpanded(shelf.projectId, !shelf.expanded)) }) {
            Text(language.text("片づけ済み"))
            Text(if (shelf.expanded) "⌄" else "›")
        }
    }
    if (!shelf.expanded) return
    items(shelf.threads, key = { "$key:${it.id.listKey}" }) { SummaryRow(it, perform) }
    if (shelf.loading) item(key = "$key:loading") { CircularProgressIndicator(Modifier.size(18.dp)) }
    shelf.error?.let { error ->
        item(key = "$key:error") {
            ErrorTextView(error, color = Color.Unspecified)
            TextButton(onClick = { perform(Intent.SetSettledExpanded(shelf.projectId, true)) }) {
                Text(language.text("再試行"))
            }
        }
    }
    if (shelf.hasMore)
        item(key = "$key:more") {
            TextButton(onClick = { perform(Intent.ExpandSettled(shelf.projectId)) }, enabled = !shelf.loading) {
                Text(language.text("もっと見る"))
            }
        }
}
