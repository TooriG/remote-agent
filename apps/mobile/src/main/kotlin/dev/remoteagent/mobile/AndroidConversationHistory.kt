package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import dev.remoteagent.core.HistoryControl
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.SessionRef

internal fun LazyListScope.historyControl(
    session: SessionRef?,
    control: HistoryControl?,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    if (session == null || control == null) return
    item(key = "history:control") {
        Column {
            control.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (control.hasMore || control.error != null) {
                TextButton(onClick = { perform(Intent.ReadOlder(session)) {} }, enabled = !control.loading) {
                    Text(if (control.hasMore) control.loadLabel else "再試行")
                }
            }
        }
    }
}
