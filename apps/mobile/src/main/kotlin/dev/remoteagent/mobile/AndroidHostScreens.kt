package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
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

@Composable
internal fun ProfilesScreen(profiles: List<HostProfile>, select: (String) -> Unit, pair: () -> Unit) {
    val language = LocalUiLanguage.current
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item { Button(onClick = pair) { Text(language.text("PCを追加")) } }
        items(profiles, key = { it.id }) { profile ->
            Card(onClick = { select(profile.id) }, modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Text(profile.name)
                    Text(profile.id, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
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
