package dev.remoteagent.mobile

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.PasswordVisualTransformation
import dev.remoteagent.core.CredentialControl
import dev.remoteagent.core.Intent
import dev.remoteagent.core.KeyUpdate
import dev.remoteagent.core.OperationKey
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.ReadCredentials
import dev.remoteagent.core.Snapshot

@Composable
internal fun CredentialSettings(snapshot: Snapshot, perform: (Intent, (Result<Outcome>) -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    TextButton(
        onClick = {
            open = true
            perform(Intent.ReadDshCredentials(ReadCredentials())) {}
        }
    ) {
        Text("接続先のAPIキーを管理")
    }
    if (open) CredentialEditor(snapshot, perform) { open = false }
}

@Composable
private fun CredentialEditor(
    snapshot: Snapshot,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    dismiss: () -> Unit,
) {
    val controls = snapshot.credentialControls()
    var provider by remember { mutableStateOf(controls.first().provider) }
    val selected = controls.first { it.provider == provider }
    val disabled = !snapshot.connected() || snapshot.operationRunning(OperationKey.DshCredentials)
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("接続先のAPIキー") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                controls.forEach { route ->
                    TextButton(onClick = { provider = route.provider }, enabled = !disabled) {
                        Text("${if (route.provider == provider) "✓ " else ""}${route.label}")
                    }
                }
                CredentialActions(selected, disabled, perform)
                snapshot.operationError(OperationKey.DshCredentials)?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = { TextButton(onClick = dismiss) { Text("完了") } },
    )
}

@Composable
private fun CredentialActions(
    selected: CredentialControl,
    disabled: Boolean,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    var key by remember(selected.provider) { mutableStateOf("") }
    val openLink = rememberExternalLinkOpener()
    Text(selected.status)
    TextButton(onClick = { openLink(selected.keyUrl) }) { Text("APIキーを取得") }
    OutlinedTextField(
        value = key,
        onValueChange = { key = it },
        label = { Text("APIキー") },
        visualTransformation = PasswordVisualTransformation(),
        singleLine = true,
        enabled = !disabled,
        modifier = Modifier.fillMaxWidth(),
    )
    TextButton(
        onClick = {
            perform(Intent.UpdateDshCredential(KeyUpdate(selected.provider, key))) {}
            key = ""
        },
        enabled = !disabled && key.isNotBlank(),
    ) {
        Text("保存")
    }
    if (selected.canRemove) {
        TextButton(
            onClick = { perform(Intent.UpdateDshCredential(KeyUpdate(selected.provider, null))) {} },
            enabled = !disabled,
        ) {
            Text("この接続先のキーを削除")
        }
    }
}
