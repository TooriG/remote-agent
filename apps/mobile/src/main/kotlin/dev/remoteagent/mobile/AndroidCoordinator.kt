package dev.remoteagent.mobile

import android.annotation.SuppressLint
import android.graphics.Bitmap
import android.os.Bundle
import android.webkit.CookieManager
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/** The existing cloud coordinator stays in BEX, with a persistent WebView login. */
@SuppressLint("SetJavaScriptEnabled")
@Composable
internal fun CoordinatorScreen(url: String, close: () -> Unit) {
    var browser by remember { mutableStateOf<WebView?>(null) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    val history = rememberSaveable(url) { Bundle() }
    Dialog(
        onDismissRequest = close,
        properties = DialogProperties(usePlatformDefaultWidth = false, dismissOnBackPress = false),
    ) {
        BackHandler {
            val view = browser
            if (view?.canGoBack() == true) view.goBack() else close()
        }
        Surface(Modifier.fillMaxSize()) {
            Column {
                Row(Modifier.fillMaxWidth()) {
                    TextButton(onClick = close) { Text("一覧に戻る") }
                    Text("メイン会話", Modifier.weight(1f))
                    TextButton(onClick = { browser?.reload() }) { Text("再読み込み") }
                }
                if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
                error?.let { Text("表示できません: $it。再読み込みしてください。") }
                AndroidView(
                    modifier = Modifier.fillMaxWidth().weight(1f),
                    factory = { context ->
                        WebView(context).apply {
                            settings.javaScriptEnabled = true
                            settings.domStorageEnabled = true
                            settings.allowFileAccess = false
                            settings.allowContentAccess = false
                            CookieManager.getInstance().setAcceptCookie(true)
                            webViewClient =
                                object : WebViewClient() {
                                    override fun shouldOverrideUrlLoading(
                                        view: WebView,
                                        request: WebResourceRequest,
                                    ): Boolean = request.url.scheme != "https"

                                    override fun onPageStarted(view: WebView, url: String?, favicon: Bitmap?) {
                                        loading = true
                                        error = null
                                    }

                                    override fun onPageFinished(view: WebView, url: String?) {
                                        loading = false
                                        view.saveState(history)
                                        CookieManager.getInstance().flush()
                                    }

                                    override fun onReceivedError(
                                        view: WebView,
                                        request: WebResourceRequest,
                                        failure: WebResourceError,
                                    ) {
                                        if (request.isForMainFrame) {
                                            loading = false
                                            error = failure.description.toString()
                                        }
                                    }
                                }
                            browser = this
                            if (restoreState(history) == null) loadUrl(url)
                        }
                    },
                    onRelease = { view ->
                        view.saveState(history)
                        CookieManager.getInstance().flush()
                        browser = null
                        view.stopLoading()
                        view.destroy()
                    },
                )
            }
        }
    }
}
