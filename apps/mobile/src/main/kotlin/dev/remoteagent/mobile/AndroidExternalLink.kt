package dev.remoteagent.mobile

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import dev.remoteagent.core.localizedUiFormat

/** Launch external links on Android instead of silently swallowing URI-handler failures. */
@Composable
internal fun rememberExternalLinkOpener(): (String) -> Unit {
    val language = LocalUiLanguage.current
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var failed by remember { mutableStateOf<String?>(null) }
    failed?.let { url ->
        AlertDialog(
            onDismissRequest = { failed = null },
            title = { Text(language.text("リンクを開けません")) },
            text = { Text(localizedUiFormat(language, "対応するブラウザーやアプリを確認してください。\n{0}", listOf(url))) },
            confirmButton = { TextButton(onClick = { failed = null }) { Text(language.text("閉じる")) } },
            dismissButton = {
                TextButton(onClick = { clipboard.setText(AnnotatedString(url)) }) { Text(language.text("リンクをコピー")) }
            },
        )
    }
    return { url ->
        val intent =
            Intent(Intent.ACTION_VIEW, Uri.parse(url))
                .addCategory(Intent.CATEGORY_BROWSABLE)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            failed = url
        } catch (_: SecurityException) {
            failed = url
        } catch (_: IllegalArgumentException) {
            failed = url
        }
    }
}
