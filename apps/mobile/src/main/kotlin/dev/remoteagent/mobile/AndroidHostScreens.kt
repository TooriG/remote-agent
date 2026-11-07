package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

@Composable
internal fun PairingScreen(
    busy: Boolean,
    pair: (String) -> Unit,
    showHosts: () -> Unit,
    scan: (onContents: (String) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    var contents by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(language.text("PCとペアリング"), style = MaterialTheme.typography.headlineMedium)
        Text(language.text("PCのBexに表示されたQRコードを読み取ります。QRの内容はこの端末に保存しません。"))
        Button(onClick = { scan { value -> contents = value } }) { Text(language.text("QRコードを読み取る")) }
        OutlinedTextField(
            contents,
            { contents = it },
            Modifier.fillMaxWidth(),
            label = { Text(language.text("ペアリングQR（手入力）")) },
            minLines = 3,
        )
        Button(onClick = { pair(contents) }, enabled = contents.isNotBlank() && !busy) { Text(language.text("ペアリング")) }
        Button(onClick = showHosts) { Text(language.text("戻る")) }
    }
}

internal data class HostConnectionState(val enabled: Boolean, val connected: Boolean)

@Composable
internal fun ProfilesScreen(
    profiles: List<Pair<HostProfile, HostConnectionState?>>,
    setEnabled: (String, Boolean) -> Unit,
    remove: (String) -> Unit,
    open: (String) -> Unit,
    pair: () -> Unit,
) {
    val language = LocalUiLanguage.current
    var removing by remember { mutableStateOf<HostProfile?>(null) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item { Button(onClick = pair) { Text(language.text("PCを追加")) } }
        items(profiles, key = { it.first.id }) { (profile, state) ->
            Card(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            TextButton(onClick = { open(profile.id) }) { Text(profile.name) }
                            Text(
                                state?.let {
                                    dev.remoteagent.core.connectionStatusLabel(language, it.connected, it.enabled)
                                } ?: language.text("読み込み中…"),
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        Switch(
                            checked = state?.enabled == true,
                            onCheckedChange = { setEnabled(profile.id, it) },
                            enabled = state != null,
                            modifier =
                                Modifier.testTag("connection.enabled.${profile.id}").semantics {
                                    contentDescription = profile.name
                                },
                        )
                    }
                    TextButton(onClick = { removing = profile }) { Text(language.text("保存した接続を削除")) }
                }
            }
        }
    }
    removing?.let { profile ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(profile.name) },
            text = { Text(language.text("このPCとの接続を解除しますか？")) },
            confirmButton = {
                TextButton(
                    onClick = {
                        removing = null
                        remove(profile.id)
                    }
                ) {
                    Text(language.text("接続を解除"))
                }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(language.text("キャンセル")) } },
        )
    }
}

@Composable
internal fun ConnectionStatus(notice: String?, busy: Boolean, reconnect: Boolean, connect: () -> Unit) {
    val language = LocalUiLanguage.current
    notice?.let { Text(language.text(it), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp)) }
    if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    if (reconnect) Button(onClick = connect, enabled = !busy) { Text(language.text("再接続")) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun HostAppBar(openSettings: () -> Unit) {
    val language = LocalUiLanguage.current
    TopAppBar(
        title = {
            androidx.compose.foundation.Image(
                androidx.compose.ui.res.painterResource(R.drawable.agent_icon),
                "Bex",
                Modifier.size(26.dp),
            )
        },
        actions = { androidx.compose.material3.TextButton(onClick = openSettings) { Text(language.text("設定")) } },
    )
}
