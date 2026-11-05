package dev.remoteagent.mobile

import android.content.Intent as AndroidIntent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.Account
import dev.remoteagent.core.AccountLogin
import dev.remoteagent.core.CancelAccountLogin
import dev.remoteagent.core.ErrorScope
import dev.remoteagent.core.Intent
import dev.remoteagent.core.ListAccounts
import dev.remoteagent.core.LogoutAccount
import dev.remoteagent.core.ManageCodexCli
import dev.remoteagent.core.OperationKey
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.ProviderKind
import dev.remoteagent.core.ReadAccountLogin
import dev.remoteagent.core.ReadCredentials
import dev.remoteagent.core.ReadHostSoftware
import dev.remoteagent.core.ReadProviderSettings
import dev.remoteagent.core.SelectAccount
import dev.remoteagent.core.SetProviderEnabled
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.StartAccountLogin
import dev.remoteagent.core.SubmitAccountLogin
import kotlinx.coroutines.delay

private const val ACCOUNT_LOGIN_POLL_INTERVAL_MILLIS = 2000L

@Composable
internal fun EnvironmentSettingsScreen(
    profile: HostProfile,
    snapshot: Snapshot,
    setEnabled: (String, Boolean) -> Unit,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    back: () -> Unit,
) {
    val language = LocalUiLanguage.current
    var provider by remember(profile.id) { mutableStateOf<ProviderKind?>(null) }
    BackHandler(provider != null) { provider = null }
    LaunchedEffect(profile.id, snapshot.connected()) {
        if (snapshot.connected()) {
            perform(Intent.ReadHostSoftware(ReadHostSoftware())) {}
            perform(Intent.ReadProviderSettings(ReadProviderSettings())) {}
        }
    }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        TextButton(onClick = { if (provider == null) back() else provider = null }) { Text(language.text("戻る")) }
        Text(profile.name, style = MaterialTheme.typography.headlineMedium)
        ErrorNotices(snapshot.errorNotices(ErrorScope.Settings), perform)
        if (provider == null) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Column {
                    Text(language.text("接続"))
                    Text(language.text(snapshot.connectionStatusLabel()), style = MaterialTheme.typography.bodySmall)
                }
                Switch(
                    snapshot.connectionEnabled(),
                    { setEnabled(profile.id, it) },
                    Modifier.testTag("environment.enabled"),
                )
            }
            HostSoftwareSettings(snapshot)
            ProviderSettingsList(snapshot, { provider = it }, perform)
        } else {
            val selected = provider ?: return@Column
            Text(
                snapshot.connectionSetup().agents.first { it.provider == selected }.name,
                style = MaterialTheme.typography.titleLarge,
            )
            ProviderSettingsContent(snapshot, selected, perform)
        }
    }
}

@Composable
private fun ProviderSettingsList(
    snapshot: Snapshot,
    choose: (ProviderKind) -> Unit,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    Text(language.text("プロバイダー"), style = MaterialTheme.typography.titleMedium)
    snapshot.connectionSetup().agents.forEach { agent ->
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            TextButton(onClick = { choose(agent.provider) }, modifier = Modifier.weight(1f)) {
                Column(Modifier.fillMaxWidth()) {
                    Text(agent.name)
                    Text(language.text(agent.label), style = MaterialTheme.typography.bodySmall)
                }
            }
            Switch(
                agent.enabled,
                { perform(Intent.SetProviderEnabled(SetProviderEnabled(agent.provider, it))) {} },
                enabled =
                    snapshot.connected() &&
                        snapshot.providerSettingsLoaded() &&
                        !snapshot.operationRunning(OperationKey.ProviderSettings),
                modifier = Modifier.testTag("provider.enabled.${agent.provider}"),
            )
        }
    }
    TextButton(
        onClick = { perform(Intent.ReadProviderSettings(ReadProviderSettings())) {} },
        enabled = snapshot.connected(),
    ) {
        Text(language.text("状態を確認"))
    }
}

@Composable
private fun HostSoftwareSettings(snapshot: Snapshot) {
    val language = LocalUiLanguage.current
    val context = LocalContext.current
    Text("Bex Host", style = MaterialTheme.typography.titleMedium)
    snapshot.hostSoftware()?.let { software ->
        Text(software.version)
        if (software.build != 0uL) Text("${language.text("ビルド")}: ${software.build}")
        Text(software.revision, style = MaterialTheme.typography.bodySmall)
    }
    TextButton(
        onClick = {
            context.startActivity(
                AndroidIntent(AndroidIntent.ACTION_VIEW, Uri.parse("https://github.com/ttizze/remote-agent/releases"))
            )
        }
    ) {
        Text(language.text("アップデートを確認"))
    }
    Text(language.text("更新はこのPCでBexを開いて適用できます。"), style = MaterialTheme.typography.bodySmall)
}

@Composable
private fun ProviderSettingsContent(
    snapshot: Snapshot,
    provider: ProviderKind,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    LaunchedEffect(provider, snapshot.connected()) {
        if (snapshot.connected()) {
            perform(Intent.ListAccounts(ListAccounts())) {}
            if (provider == ProviderKind.CODEX) perform(Intent.ManageCodexCli(ManageCodexCli(false))) {}
            if (provider == ProviderKind.DEEP_SEEK) perform(Intent.ReadDshCredentials(ReadCredentials())) {}
        }
    }
    val login = snapshot.accountLogin()?.takeIf { it.provider == provider }
    if (provider == ProviderKind.DEEP_SEEK) CredentialSettings(snapshot, perform)
    else if (login != null) AccountLoginSettings(login, snapshot.connected(), perform)
    else AccountSettings(snapshot, provider, perform)
    ProviderRuntimeSettings(snapshot, provider, perform)
}

@Composable
private fun AccountSettings(
    snapshot: Snapshot,
    provider: ProviderKind,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    var signingOut by remember(provider) { mutableStateOf<Account?>(null) }
    Text(language.text("アカウント"), style = MaterialTheme.typography.titleMedium)
    snapshot
        .accounts()
        ?.accounts
        ?.filter { it.provider == provider }
        ?.forEach { account ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(
                        onClick = { perform(Intent.SelectAccount(SelectAccount(provider, account.id))) {} },
                        enabled = snapshot.connected(),
                    ) {
                        val mark = if (snapshot.accountIsSelected(provider, account.id)) "✓ " else ""
                        Text(mark + (account.email ?: account.id))
                    }
                    account.planType?.let { Text(it) }
                    account.usage?.let { usage ->
                        usage.windows.forEach { window -> Text("${window.label}: ${window.remainingPercent}%") }
                        usage.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    }
                    TextButton(onClick = { signingOut = account }, enabled = snapshot.connected()) {
                        Text(language.text("サインアウト"))
                    }
                }
            }
        }
    TextButton(onClick = { perform(Intent.ListAccounts(ListAccounts())) {} }, enabled = snapshot.connected()) {
        Text(language.text("再取得"))
    }
    TextButton(
        onClick = { perform(Intent.StartAccountLogin(StartAccountLogin(provider))) {} },
        enabled = snapshot.connected(),
    ) {
        Text(language.text("アカウントを追加"))
    }
    signingOut?.let { account ->
        AlertDialog(
            onDismissRequest = { signingOut = null },
            title = { Text(language.text("サインアウトしますか？")) },
            confirmButton = {
                TextButton(
                    onClick = {
                        signingOut = null
                        perform(Intent.LogoutAccount(LogoutAccount(provider, account.id))) {}
                    }
                ) {
                    Text(language.text("サインアウト"))
                }
            },
            dismissButton = { TextButton(onClick = { signingOut = null }) { Text(language.text("キャンセル")) } },
        )
    }
}

@Composable
private fun AccountLoginSettings(
    login: AccountLogin,
    connected: Boolean,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    val context = LocalContext.current
    var code by remember(login.loginId) { mutableStateOf("") }
    LaunchedEffect(login.loginId, connected) {
        while (connected) {
            delay(ACCOUNT_LOGIN_POLL_INTERVAL_MILLIS)
            perform(Intent.ReadAccountLogin(ReadAccountLogin(login.provider, login.loginId, null))) {}
        }
    }
    Text(login.userCode)
    TextButton(
        onClick = { context.startActivity(AndroidIntent(AndroidIntent.ACTION_VIEW, Uri.parse(login.verificationUrl))) }
    ) {
        Text(language.text("認証ページを開く"))
    }
    if (login.requiresCodeSubmission) {
        OutlinedTextField(
            code,
            { code = it },
            label = { Text(language.text("認証コード")) },
            modifier = Modifier.fillMaxWidth(),
        )
        TextButton(
            onClick = {
                val submitted = code
                code = ""
                perform(Intent.SubmitAccountLogin(SubmitAccountLogin(login.provider, login.loginId, submitted))) {}
            },
            enabled = connected && code.isNotBlank(),
        ) {
            Text(language.text("送信"))
        }
    }
    TextButton(onClick = { perform(Intent.CancelAccountLogin(CancelAccountLogin(login.provider, login.loginId))) {} }) {
        Text(language.text("キャンセル"))
    }
}

@Composable
private fun ProviderRuntimeSettings(
    snapshot: Snapshot,
    provider: ProviderKind,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    if (provider != ProviderKind.CODEX) {
        Text(language.text("このプロバイダーはBexと一緒に更新されます。"))
        return
    }
    Text("Codex CLI", style = MaterialTheme.typography.titleMedium)
    val status = snapshot.codexCliStatus()
    status?.version?.let { Text(it) }
    val enabled = snapshot.connected() && !snapshot.operationRunning(OperationKey.CodexCli)
    TextButton(onClick = { perform(Intent.ManageCodexCli(ManageCodexCli(false))) {} }, enabled = enabled) {
        Text(language.text("状態を確認"))
    }
    TextButton(onClick = { perform(Intent.ManageCodexCli(ManageCodexCli(true))) {} }, enabled = enabled) {
        Text(language.text("最新版に更新"))
    }
    if (status?.restartRequired == true) Text(language.text("更新が完了しました。新しいCLIを使用するにはホストを再起動してください。"))
    status?.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
}
