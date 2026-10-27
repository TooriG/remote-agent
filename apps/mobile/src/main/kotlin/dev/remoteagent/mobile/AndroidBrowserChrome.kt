package dev.remoteagent.mobile

import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.remoteagent.core.BrowserCredentials
import dev.remoteagent.core.BrowserAction
import dev.remoteagent.core.BrowserTab
import dev.remoteagent.core.browserAddressUrl

@Composable
internal fun BrowserTabs(tabs: List<BrowserTab>, send: (BrowserAction) -> Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
        tabs.forEach { tab ->
            TextButton(onClick = { send(BrowserAction.SelectTab(tab.id)) }) {
                Text(if (tab.url == "about:blank") "新しいタブ" else tab.title.ifEmpty { tab.url }, maxLines = 1)
            }
            TextButton(onClick = { send(BrowserAction.CloseTab(tab.id)) }) { Text("×") }
        }
        TextButton(onClick = { send(BrowserAction.OpenTab("about:blank")) }) { Text("＋") }
    }
}

@Composable
@Suppress("LongParameterList")
internal fun BrowserToolbar(
    tab: BrowserTab?,
    enabled: Boolean,
    canGoBack: Boolean,
    canGoForward: Boolean,
    send: (BrowserAction) -> Unit,
    fit: () -> Unit,
    type: () -> Unit,
    failed: (String?) -> Unit,
) {
    val url = tab?.url?.takeUnless { it == "about:blank" } ?: ""
    var address by remember { mutableStateOf("") }
    var focused by remember { mutableStateOf(false) }
    LaunchedEffect(url, focused) { if (!focused) address = url }
    Row(
        Modifier.fillMaxWidth().height(48.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(
            onClick = { send(BrowserAction.Back) },
            enabled = canGoBack,
            modifier = Modifier.size(40.dp),
        ) {
            Icon(painterResource(R.drawable.ic_arrow_left), "前のページ", Modifier.size(16.dp))
        }
        IconButton(
            onClick = { send(BrowserAction.Forward) },
            enabled = canGoForward,
            modifier = Modifier.size(40.dp),
        ) {
            Icon(painterResource(R.drawable.ic_arrow_right), "次のページ", Modifier.size(16.dp))
        }
        IconButton(onClick = { send(BrowserAction.Reload) }, enabled = enabled, modifier = Modifier.size(40.dp)) {
            Icon(painterResource(R.drawable.ic_refresh), "再読み込み", Modifier.size(16.dp))
        }
        BasicTextField(
            address,
            { address = it },
            Modifier.weight(1f).onFocusChanged { focused = it.isFocused },
            enabled = enabled,
            singleLine = true,
            textStyle =
                MaterialTheme.typography.bodySmall.copy(fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurface),
            cursorBrush = SolidColor(MaterialTheme.colorScheme.onSurface),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions =
                KeyboardActions(
                    onGo = {
                        runCatching { browserAddressUrl(address) }
                            .onSuccess { send(BrowserAction.Navigate(it)) }
                            .onFailure { failed(it.message) }
                    }
                ),
            decorationBox = { input ->
                Box {
                    if (address.isEmpty())
                        Text("検索または URL", fontSize = 14.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    input()
                }
            },
        )
        BrowserMenu(tab?.id, enabled, send, fit, type)
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.6f))
}

@Composable
private fun BrowserMenu(
    tabId: String?,
    enabled: Boolean,
    send: (BrowserAction) -> Unit,
    fit: () -> Unit,
    type: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { menuOpen = true }, enabled = enabled, modifier = Modifier.size(40.dp)) {
            Icon(painterResource(R.drawable.ic_more), "ブラウザのメニュー", Modifier.size(16.dp))
        }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            DropdownMenuItem(text = { Text("ログイン状態・保存パスワード") }, onClick = {
                menuOpen = false
                send(BrowserAction.Credentials)
            })
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("新しいタブ") },
                onClick = {
                    menuOpen = false
                    send(BrowserAction.OpenTab("about:blank"))
                },
            )
            DropdownMenuItem(
                text = { Text("タブを閉じる") },
                onClick = {
                    menuOpen = false
                    tabId?.let { send(BrowserAction.CloseTab(it)) }
                },
            )
            DropdownMenuItem(
                text = { Text("文字入力") },
                onClick = { menuOpen = false; type() },
            )
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("画面に合わせる") },
                onClick = {
                    menuOpen = false
                    fit()
                },
            )
            DropdownMenuItem(
                text = { Text("デスクトップ · 1280 × 800") },
                onClick = {
                    menuOpen = false
                    send(BrowserAction.Resize(1280u, 800u))
                },
            )
            DropdownMenuItem(
                text = { Text("モバイル · 390 × 844") },
                onClick = {
                    menuOpen = false
                    send(BrowserAction.Resize(390u, 844u))
                },
            )
        }
    }
}

@Composable
internal fun BrowserEmptyState(connected: Boolean, modifier: Modifier) {
    Box(modifier.fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
        if (!connected)
            Text("Host のブラウザに接続中…", Modifier.padding(48.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        else
            Column(
                Modifier.widthIn(max = 384.dp).padding(horizontal = 24.dp, vertical = 48.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(24.dp),
            ) {
                Box(
                    Modifier.padding(bottom = 24.dp)
                        .size(36.dp)
                        .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(6.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        painterResource(R.drawable.ic_globe),
                        null,
                        Modifier.size(18.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text("プレビューはまだありません", fontSize = 20.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
                Text(
                    "上の欄に URL を入力するか、AI にブラウザ操作を依頼してください。",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
    }
}

@Composable
internal fun BrowserCredentialsDialog(
    credentials: BrowserCredentials,
    send: (BrowserAction) -> Unit,
    close: () -> Unit,
) {
    AlertDialog(onDismissRequest = close, title = { Text("ブラウザデータ") }, confirmButton = {
        TextButton(onClick = close) { Text("閉じる") }
    }, text = {
        Column(Modifier.verticalScroll(rememberScrollState())) {
            Text("接続先のブラウザから取り込む")
            if (credentials.profiles.isEmpty()) Text("取り込めるブラウザプロファイルがありません。")
            credentials.profiles.forEach { profile ->
                Text(profile.label)
                TextButton(
                    onClick = { send(BrowserAction.ImportCookies(profile.id)) },
                    enabled = profile.id.isNotEmpty(),
                ) { Text("ログイン状態（Cookie）") }
                TextButton(
                    onClick = { send(BrowserAction.ImportPasswords(profile.id)) },
                    enabled = profile.id.isNotEmpty(),
                ) { Text("保存パスワード → Keychain") }
            }
            credentials.origin?.let { origin ->
                HorizontalDivider()
                Text("保存パスワード · $origin")
                if (credentials.accounts.isEmpty()) {
                    Text(credentials.passwordError ?: "このサイトの保存パスワードはありません。")
                }
                credentials.accounts.forEach { username ->
                    TextButton(onClick = { send(BrowserAction.FillPassword(origin, username)) }) {
                        Text(username.ifEmpty { "ユーザー名なし" })
                    }
                }
                TextButton(
                    onClick = { send(BrowserAction.DeletePasswords(origin)) },
                    enabled = credentials.accounts.isNotEmpty(),
                ) { Text("このサイトの保存パスワードを削除") }
            }
        }
    })
}
