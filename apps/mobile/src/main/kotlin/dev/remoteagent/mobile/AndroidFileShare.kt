package dev.remoteagent.mobile

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent as AndroidIntent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.DownloadFile
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import java.io.File
import java.io.IOException
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

/** Explicit sharing only. The provider exposes this bounded cache, never Host paths. */
@Composable
internal fun rememberFileSharer(
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
    failure: (String) -> Unit,
): (String, String) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    return { path, mime ->
        scope.launch {
            val directory = File(context.cacheDir, "shared-files")
            directory.mkdirs()
            directory
                .listFiles()
                ?.filter { it.lastModified() < System.currentTimeMillis() - SHARE_CACHE_MILLIS }
                ?.forEach { it.delete() }
            val target = File(directory, "${UUID.randomUUID()}-${path.replace('\\', '/').substringAfterLast('/')}")
            var shared = false
            try {
                fileOperation(Intent.DownloadFile(DownloadFile(path, target.path)), perform)
                shareDownloadedFile(context, target, mime)
                shared = true
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: AgentException) {
                failure(error.displayMessage() ?: "ファイルを共有できません")
            } catch (error: IOException) {
                failure(error.displayMessage() ?: "ファイルを共有できません")
            } catch (error: IllegalArgumentException) {
                failure(error.displayMessage() ?: "ファイルを共有できません")
            } catch (error: SecurityException) {
                failure(error.displayMessage() ?: "ファイルを共有できません")
            } catch (error: ActivityNotFoundException) {
                failure(error.displayMessage() ?: "ファイルを共有できません")
            } finally {
                if (!shared) target.delete()
            }
        }
    }
}

private fun shareDownloadedFile(context: Context, target: File, mime: String) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", target)
    val share =
        AndroidIntent(AndroidIntent.ACTION_SEND).apply {
            type = mime
            putExtra(AndroidIntent.EXTRA_STREAM, uri)
            addFlags(AndroidIntent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = android.content.ClipData.newRawUri(null, uri)
        }
    context.startActivity(AndroidIntent.createChooser(share, null))
}

private const val SHARE_CACHE_MILLIS = 24 * 60 * 60 * 1000L
