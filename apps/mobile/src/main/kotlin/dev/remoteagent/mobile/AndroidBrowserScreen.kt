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
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.remoteagent.core.BrowserAction
import dev.remoteagent.core.BrowserController
import dev.remoteagent.core.BrowserFrame
import dev.remoteagent.core.BrowserKey
import dev.remoteagent.core.BrowserRequest
import dev.remoteagent.core.SessionRef
import dev.remoteagent.core.browserFitAction
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

@Composable
internal fun BrowserLauncher(
    thread: SessionRef?,
    profile: String?,
    browser: suspend (String?, BrowserRequest) -> BrowserFrame,
) {
    var visible by remember { mutableStateOf(false) }
    TextButton(onClick = { visible = true }, enabled = thread != null) { Text("ブラウザ") }
    if (visible && thread != null) {
        Dialog(onDismissRequest = { visible = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
            Surface(Modifier.fillMaxSize().imePadding()) { BrowserPane(thread, profile, browser) { visible = false } }
        }
    }
}

private const val BROWSER_POLL_MS = 150L
private const val BROWSER_RETRY_MS = 1500L
private const val BROWSER_RELEASE_MS = 1000L
private const val BROWSER_QUEUE_LIMIT = 64
private const val BROWSER_MAX_SCROLL = 4096f

private class BrowserSession(private val thread: SessionRef) {
    private val client = UUID.randomUUID().toString()
    private val commands = ArrayDeque<BrowserRequest>()
    var frame by mutableStateOf<BrowserFrame?>(null)
        private set

    var image by mutableStateOf<ImageBitmap?>(null)
        private set

    var error by mutableStateOf<String?>(null)
    private var followSize = true
    private var viewportWidth = 0.0
    private var viewportHeight = 0.0

    fun fitViewport() {
        followSize = true
    }

    fun setViewport(width: Double, height: Double) {
        viewportWidth = width
        viewportHeight = height
    }

    private fun request(action: BrowserAction) =
        BrowserRequest(thread, frame?.tabId ?: "", frame?.imageId ?: "", client, frame?.generation ?: 0uL, action)

    fun send(action: BrowserAction) {
        if (commands.size < BROWSER_QUEUE_LIMIT) {
            if (action is BrowserAction.Resize) followSize = false
            commands.addLast(request(action))
        }
    }

    private fun nextRequest(): BrowserRequest {
        commands.removeFirstOrNull()?.let {
            return it
        }
        val fit =
            if (followSize && frame?.controller == BrowserController.YOU)
                browserFitAction(viewportWidth, viewportHeight, frame?.width ?: 0u, frame?.height ?: 0u)
            else null
        return request(fit ?: BrowserAction.Read)
    }

    suspend fun run(profile: String?, browser: suspend (String?, BrowserRequest) -> BrowserFrame) {
        try {
            while (kotlinx.coroutines.currentCoroutineContext().isActive) {
                try {
                    val result = browser(profile, nextRequest())
                    image = decodeBrowserImage(result.image) ?: image
                    frame = result
                    error = null
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (failure: dev.remoteagent.core.AgentException) {
                    error = failure.message
                } catch (failure: IllegalStateException) {
                    error = failure.message
                }
                delay(if (error == null) BROWSER_POLL_MS else BROWSER_RETRY_MS)
            }
        } finally {
            commands.clear()
            if (frame?.controller == BrowserController.YOU) {
                withContext(NonCancellable) {
                    withTimeoutOrNull(BROWSER_RELEASE_MS) {
                        runCatching { browser(profile, request(BrowserAction.ReleaseControl)) }
                    }
                }
            }
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
private fun BrowserPane(
    thread: SessionRef,
    profile: String?,
    browser: suspend (String?, BrowserRequest) -> BrowserFrame,
    dismiss: () -> Unit,
) {
    val session = remember(thread) { BrowserSession(thread) }
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
    LaunchedEffect(thread, active) { if (active) session.run(profile, browser) }
    val frame = session.frame
    val owns = frame?.controller == BrowserController.YOU
    val tab = frame?.tabs?.firstOrNull { it.id == frame.tabId }
    val density = LocalDensity.current.density
    Column {
        Row(
            Modifier.fillMaxWidth().height(44.dp).padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("ブラウザ", Modifier.weight(1f), style = MaterialTheme.typography.labelLarge)
            IconButton(onClick = dismiss) { Icon(painterResource(R.drawable.ic_close), "閉じる", Modifier.size(16.dp)) }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.6f))
        if ((frame?.tabs?.size ?: 0) > 1) BrowserTabs(frame?.tabs.orEmpty(), owns, session::send)
        BrowserToolbar(
            tab,
            owns,
            frame?.canGoBack == true,
            frame?.canGoForward == true,
            session::send,
            session::fitViewport,
        ) {
            session.error = it
        }
        session.error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp)) }
        frame?.dialog?.let { BrowserDialogView(it, owns, session::send) }
        val blank = tab?.url == "about:blank"
        BrowserImage(
            session.image?.takeUnless { blank },
            frame != null,
            owns,
            session::send,
            Modifier.weight(1f).onSizeChanged { size ->
                session.setViewport((size.width / density).toDouble(), (size.height / density).toDouble())
            },
        )
        BrowserControlBar(frame?.controller, session::send)
        if (owns) BrowserTextInput(session::send)
    }
}

@Composable
private fun BrowserDialogView(
    dialog: dev.remoteagent.core.BrowserDialog,
    owns: Boolean,
    send: (BrowserAction) -> Unit,
) {
    var response by remember(dialog) { mutableStateOf("") }
    Text(dialog.message, Modifier.padding(12.dp))
    if (dialog.prompt) OutlinedTextField(response, { response = it }, Modifier.fillMaxWidth())
    Row {
        TextButton(onClick = { send(BrowserAction.Dialog(false, "")) }, enabled = owns) { Text("キャンセル") }
        TextButton(onClick = { send(BrowserAction.Dialog(true, response)) }, enabled = owns) { Text("OK") }
    }
}

@Composable
private fun BrowserImage(
    image: ImageBitmap?,
    connected: Boolean,
    owns: Boolean,
    send: (BrowserAction) -> Unit,
    modifier: Modifier,
) {
    if (image == null) {
        BrowserEmptyState(connected, modifier)
        return
    }
    Image(
        image,
        "Host の共有ブラウザ",
        modifier
            .fillMaxSize()
            .pointerInput(owns, image.width, image.height) {
                if (owns)
                    detectTapGestures { point ->
                        val scale = minOf(size.width.toFloat() / image.width, size.height.toFloat() / image.height)
                        val x = (point.x - (size.width - image.width * scale) / 2) / scale
                        val y = (point.y - (size.height - image.height * scale) / 2) / scale
                        if (x in 0f..<image.width.toFloat() && y in 0f..<image.height.toFloat())
                            send(BrowserAction.Click(x.toDouble(), y.toDouble()))
                    }
            }
            .pointerInput(owns, image.width, image.height) {
                var deltaX = 0f
                var deltaY = 0f
                if (owns)
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
private fun BrowserTextInput(send: (BrowserAction) -> Unit) {
    var text by remember { mutableStateOf("") }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        OutlinedTextField(
            text,
            { text = it },
            Modifier.weight(1f),
            placeholder = { Text("選択した欄に入力") },
            visualTransformation = PasswordVisualTransformation(),
        )
        TextButton(
            onClick = {
                send(BrowserAction.Type(text))
                text = ""
            },
            enabled = text.isNotEmpty(),
        ) {
            Text("入力")
        }
        TextButton(onClick = { send(BrowserAction.Key(BrowserKey.ENTER)) }) { Text("Enter") }
    }
}
