package dev.remoteagent.mobile

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.BrowserAction
import dev.remoteagent.core.BrowserCredentials
import dev.remoteagent.core.BrowserFrame
import dev.remoteagent.core.BrowserInputKind
import dev.remoteagent.core.BrowserKey
import dev.remoteagent.core.BrowserStatus
import dev.remoteagent.core.BrowserUpdate
import dev.remoteagent.core.SessionRef
import dev.remoteagent.core.browserFitAction
import dev.remoteagent.core.browserInputFocus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

@Composable
internal fun BrowserLauncher(thread: SessionRef?, store: AgentStore?) {
    val language = LocalUiLanguage.current
    var visible by remember { mutableStateOf(false) }
    TextButton(onClick = { visible = true }, enabled = thread != null) { Text(language.text("ブラウザ")) }
    if (visible && thread != null) {
        Dialog(onDismissRequest = { visible = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
            Surface(Modifier.fillMaxSize().imePadding()) { BrowserPane(thread, store) { visible = false } }
        }
    }
}

private const val BROWSER_MAX_SCROLL = 4096f

/** What the pane shows; core orders the actions and reads the Host page. */
private class BrowserViewState(private val session: dev.remoteagent.core.BrowserSession?) {
    var frame by mutableStateOf<BrowserFrame?>(null)
        private set

    var image by mutableStateOf<ImageBitmap?>(null)
        private set

    var error by mutableStateOf<String?>(null)
    var connectionError by mutableStateOf<String?>(null)
    var notice by mutableStateOf<BrowserStatus?>(null)
    var credentials by mutableStateOf<BrowserCredentials?>(null)
    var keyboardRequest by mutableStateOf(0)
        private set

    private var viewportWidth = 0.0
    private var viewportHeight = 0.0

    fun toggleKeyboard() {
        keyboardRequest = if (keyboardRequest == 0) 1 else 0
    }

    fun fitViewport() {
        browserFitAction(viewportWidth, viewportHeight, frame?.width ?: 0u, frame?.height ?: 0u)?.let(::send)
    }

    fun setViewport(width: Double, height: Double) {
        viewportWidth = width
        viewportHeight = height
    }

    fun send(action: BrowserAction) {
        error = null
        credentials = null
        notice =
            if (action is BrowserAction.ImportCookies || action is BrowserAction.ImportPasswords)
                BrowserStatus.Importing(remote = true)
            else null
        session?.send(action)
    }

    private suspend fun apply(result: BrowserFrame, action: BrowserAction) {
        image =
            if (result.tabId.isEmpty() || result.imageId.isEmpty()) null else decodeBrowserImage(result.image) ?: image
        frame = result
        credentials = result.credentials ?: credentials
        notice =
            result.notice?.let { BrowserStatus.Notice(it) }
                ?: result.value.takeIf(String::isNotEmpty)?.let { BrowserStatus.Value(it) }
                ?: notice
        connectionError = null
        browserInputFocus(action, result.inputKind)?.let { focus ->
            keyboardRequest = if (focus) keyboardRequest + 1 else 0
        }
    }

    suspend fun run() {
        val session = session ?: return run { connectionError = "ホストに接続してください。" }
        session.setActive(true)
        try {
            while (true) {
                when (val update = session.next() ?: break) {
                    is BrowserUpdate.Frame -> apply(update.frame, update.action)
                    is BrowserUpdate.ReadFailed -> connectionError = update.message
                    is BrowserUpdate.ActionFailed -> {
                        error = update.message
                        notice = null
                    }
                }
            }
        } finally {
            session.setActive(false)
            session.clear()
        }
    }
}

private suspend fun decodeBrowserImage(bytes: ByteArray): ImageBitmap? =
    if (bytes.isEmpty()) null
    else
        withContext(Dispatchers.Default) {
            checkNotNull(BitmapFactory.decodeByteArray(bytes, 0, bytes.size)).asImageBitmap()
        }

@Composable
private fun BrowserPane(thread: SessionRef, store: AgentStore?, dismiss: () -> Unit) {
    // Each Store owns its browser: a newly ready or re-paired Store starts a new session.
    val session = remember(thread, store) { BrowserViewState(store?.browserSession(thread)) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var active by remember { mutableStateOf(lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) }
    DisposableEffect(lifecycle) {
        val observer =
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    active = true
                }

                override fun onStop(owner: LifecycleOwner) {
                    active = false
                }
            }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(session, active) { if (active) session.run() }
    session.credentials?.let { BrowserCredentialsDialog(it, session::send) { session.credentials = null } }
    val frame = session.frame
    val enabled = frame != null && active && session.connectionError == null
    val tab = frame?.tabs?.firstOrNull { it.id == frame.tabId }
    val density = LocalDensity.current.density
    val colors = MaterialTheme.colorScheme
    Column {
        BrowserHeader(dismiss)
        HorizontalDivider(color = colors.outlineVariant.copy(alpha = 0.6f))
        if ((frame?.tabs?.size ?: 0) > 1) BrowserTabs(frame?.tabs.orEmpty(), session::send)
        BrowserToolbar(
            tab,
            enabled,
            frame?.canGoBack == true,
            frame?.canGoForward == true,
            session::send,
            session::fitViewport,
            session::toggleKeyboard,
        ) {
            session.error = it
        }
        BrowserNotice(session.error ?: session.connectionError, session.notice)
        frame?.dialog?.let { BrowserDialogView(it, enabled, session::send) }
        val blank = tab == null || tab.url == "about:blank"
        BrowserImage(
            session.image?.takeUnless { blank },
            frame != null,
            enabled,
            session::send,
            Modifier.weight(1f).onSizeChanged { size ->
                session.setViewport((size.width / density).toDouble(), (size.height / density).toDouble())
            },
        )
        if (session.keyboardRequest > 0) {
            BrowserTextInput(frame?.inputKind, session.keyboardRequest, enabled, session::send)
        }
    }
}

@Composable
private fun BrowserDialogView(
    dialog: dev.remoteagent.core.BrowserDialog,
    enabled: Boolean,
    send: (BrowserAction) -> Unit,
) {
    val language = LocalUiLanguage.current
    var response by remember(dialog) { mutableStateOf("") }
    Text(dialog.message, Modifier.padding(12.dp))
    if (dialog.prompt) OutlinedTextField(response, { response = it }, Modifier.fillMaxWidth())
    Row {
        TextButton(onClick = { send(BrowserAction.Dialog(false, "")) }, enabled = enabled) {
            Text(language.text("キャンセル"))
        }
        TextButton(onClick = { send(BrowserAction.Dialog(true, response)) }, enabled = enabled) { Text("OK") }
    }
}

@Composable
private fun BrowserImage(
    image: ImageBitmap?,
    connected: Boolean,
    enabled: Boolean,
    send: (BrowserAction) -> Unit,
    modifier: Modifier,
) {
    val language = LocalUiLanguage.current
    if (image == null) {
        BrowserEmptyState(connected, modifier)
        return
    }
    Image(
        image,
        language.text("ホストの共有ブラウザ"),
        modifier
            .fillMaxSize()
            .pointerInput(enabled, image.width, image.height) {
                if (enabled)
                    detectTapGestures { point ->
                        val scale = minOf(size.width.toFloat() / image.width, size.height.toFloat() / image.height)
                        val x = (point.x - (size.width - image.width * scale) / 2) / scale
                        val y = (point.y - (size.height - image.height * scale) / 2) / scale
                        if (x in 0f..<image.width.toFloat() && y in 0f..<image.height.toFloat())
                            send(BrowserAction.Click(x.toDouble(), y.toDouble()))
                    }
            }
            .pointerInput(enabled, image.width, image.height) {
                var deltaX = 0f
                var deltaY = 0f
                if (enabled)
                    detectDragGestures(
                        onDragStart = {
                            deltaX = 0f
                            deltaY = 0f
                        },
                        onDragEnd = {
                            val scale = minOf(size.width.toFloat() / image.width, size.height.toFloat() / image.height)
                            send(
                                BrowserAction.Scroll(
                                    image.width / 2.0,
                                    image.height / 2.0,
                                    (-deltaX / scale).coerceIn(-BROWSER_MAX_SCROLL, BROWSER_MAX_SCROLL).toDouble(),
                                    (-deltaY / scale).coerceIn(-BROWSER_MAX_SCROLL, BROWSER_MAX_SCROLL).toDouble(),
                                )
                            )
                        },
                    ) { change, amount ->
                        change.consume()
                        deltaX += amount.x
                        deltaY += amount.y
                    }
            },
        contentScale = ContentScale.Fit,
    )
}

@Composable
private fun BrowserTextInput(
    kind: BrowserInputKind?,
    focusRequest: Int,
    enabled: Boolean,
    send: (BrowserAction) -> Unit,
) {
    val language = LocalUiLanguage.current
    var text by remember(kind) { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    val keyboard = LocalSoftwareKeyboardController.current
    val submit = {
        if (enabled && text.isNotEmpty()) {
            send(BrowserAction.Type(text))
            text = ""
        }
    }
    LaunchedEffect(focusRequest) {
        focus.requestFocus()
        keyboard?.show()
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = { send(BrowserAction.Key(BrowserKey.TAB)) }, enabled = enabled) { Text("Tab") }
            TextButton(onClick = { send(BrowserAction.Key(BrowserKey.SELECT_ALL)) }, enabled = enabled) {
                Text(language.text("すべて選択"))
            }
            TextButton(onClick = { send(BrowserAction.Key(BrowserKey.BACKSPACE)) }, enabled = enabled) {
                Text(language.text("1文字削除"))
            }
            TextButton(onClick = { send(BrowserAction.Key(BrowserKey.ENTER)) }, enabled = enabled) { Text("Enter") }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                text,
                { text = it },
                Modifier.weight(1f).focusRequester(focus),
                enabled = enabled,
                placeholder = { Text(language.text("選択した欄に入力")) },
                visualTransformation =
                    if (kind == BrowserInputKind.TEXT) VisualTransformation.None else PasswordVisualTransformation(),
                keyboardOptions =
                    KeyboardOptions(
                        keyboardType = if (kind == BrowserInputKind.TEXT) KeyboardType.Text else KeyboardType.Password,
                        imeAction = ImeAction.Done,
                    ),
                keyboardActions = KeyboardActions(onDone = { submit() }),
                maxLines = 4,
            )
            TextButton(
                onClick = submit,
                enabled = enabled && text.isNotEmpty(),
            ) {
                Text(language.text("入力"))
            }
        }
    }
}

@Composable
private fun BrowserHeader(dismiss: () -> Unit) {
    val language = LocalUiLanguage.current
    Row(
        Modifier.fillMaxWidth().height(44.dp).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(language.text("ブラウザ"), Modifier.weight(1f), style = MaterialTheme.typography.labelLarge)
        IconButton(onClick = dismiss) {
            Icon(painterResource(R.drawable.ic_close), language.text("閉じる"), Modifier.size(16.dp))
        }
    }
}
