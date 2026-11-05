package dev.remoteagent.mobile

import android.content.Context
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.Intent
import dev.remoteagent.core.MarkdownFileReference
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.ReadFile
import dev.remoteagent.core.fileHostPath
import dev.remoteagent.core.filePreviewMimeType
import dev.remoteagent.core.filePreviewTruncationNotice
import dev.remoteagent.core.mobileFileOpenRequest
import java.io.IOException
import kotlinx.coroutines.CancellationException

private data class FileViewerState(
    val path: String,
    val directory: Boolean = false,
    val text: String? = null,
    val truncated: Boolean = false,
    val loading: Boolean = false,
    val error: String? = null,
)

/** Text stays in the viewer; binary files download through the Host and open in a native app. */
@Composable
internal fun ConversationFileViewer(
    reference: MarkdownFileReference,
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
    source: String? = null,
    workspaceRoot: String? = null,
    dismiss: () -> Unit,
) {
    val files = LocalFiles.current
    val location = LocalFileLocation.current
    val context = LocalContext.current
    val cwd = workspaceRoot ?: location.cwd
    val home = location.home
    val initialPath = fileHostPath(reference.path, home)
    val request = mobileFileOpenRequest(source ?: "", cwd, home)
    var file by remember(reference.path) { mutableStateOf(FileViewerState(initialPath, loading = true)) }
    var attempt by remember(reference.path) { mutableStateOf(0) }
    val share = rememberFileSharer(perform) { file = file.copy(error = it) }
    LaunchedEffect(reference.path, attempt) {
        file = file.copy(loading = true, error = null)
        val failure =
            try {
                val resolved = request?.let {
                    fileOperation(Intent.ResolveFileReference(it), perform) as Outcome.FileReferenceResolved
                }
                file =
                    FileViewerState(
                        resolved?.path ?: initialPath,
                        directory = resolved?.directory == true,
                        loading = true,
                    )
                val content = loadFileViewer(file.path, file.directory, context, perform)
                if (content == null) dismiss() else file = content
                null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: AgentException) {
                failure
            } catch (failure: IllegalStateException) {
                failure
            } catch (failure: IOException) {
                failure
            }
        file = file.copy(loading = false, error = failure?.let { it.displayMessage() ?: "ファイルを開けません" })
    }
    if (file.directory) {
        WorkspaceFilesDialog(files, file.path, checkNotNull(perform), dismiss)
    } else {
        AlertDialog(
            onDismissRequest = dismiss,
            title = { Text(reference.label) },
            text = { FileViewerBody(file, reference.line, perform) { share(file.path, "application/octet-stream") } },
            confirmButton = { TextButton(onClick = dismiss) { Text(LocalUiLanguage.current.text("閉じる")) } },
            dismissButton = {
                if (file.error != null)
                    TextButton(onClick = { attempt++ }) { Text(LocalUiLanguage.current.text("再試行")) }
            },
        )
    }
}

/** Null means a native app accepted the downloaded file; other results stay in this viewer. */
private suspend fun loadFileViewer(
    path: String,
    directory: Boolean,
    context: Context,
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
): FileViewerState? {
    if (directory) return FileViewerState(path, directory = true)
    val mime = filePreviewMimeType(path)
    return if (mime != null) {
        exportHostFile(context, path, mime, perform, FileExportAction.Open)
        null
    } else {
        val result = fileOperation(Intent.ReadFile(ReadFile(path, false)), perform) as Outcome.FileRead
        FileViewerState(path, text = result.file.text, truncated = result.file.truncated)
    }
}

@Composable
private fun FileViewerBody(
    file: FileViewerState,
    line: UInt?,
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
    download: () -> Unit,
) {
    val path = file.path
    val text = file.text
    val error = file.error
    if (file.loading) CircularProgressIndicator()
    else
        Column(Modifier.heightIn(max = 480.dp)) {
            SelectionContainer { Text(path) }
            if (file.truncated) Text(LocalUiLanguage.current.text(filePreviewTruncationNotice()))
            error?.let { Text(it) }
            if (file.truncated || error != null) {
                TextButton(onClick = download) { Text(LocalUiLanguage.current.text("ダウンロード")) }
            }
            if (text != null && error == null) {
                if (
                    line == null &&
                        dev.remoteagent.core.markdownFileKind(path) == dev.remoteagent.core.MarkdownFileKind.MARKDOWN
                ) {
                    Column(Modifier.heightIn(max = 400.dp).verticalScroll(rememberScrollState())) {
                        PreviewText(text, true, path.replace('\\', '/').substringBeforeLast('/'), perform)
                    }
                } else FileViewerText(text, line)
            }
        }
}

@Composable
private fun FileViewerText(text: String, line: UInt?) {
    val scroll = rememberScrollState()
    var lineTop by remember(text, line) { mutableStateOf<Int?>(null) }
    LaunchedEffect(lineTop) { lineTop?.let { scroll.scrollTo(it) } }
    SelectionContainer {
        Text(
            text,
            Modifier.heightIn(max = 400.dp).verticalScroll(scroll).horizontalScroll(rememberScrollState()),
            fontFamily = FontFamily.Monospace,
            onTextLayout = { layout ->
                if (line != null && layout.lineCount > 0) {
                    lineTop =
                        layout
                            .getLineTop((line.toLong() - 1).coerceIn(0, layout.lineCount.toLong() - 1).toInt())
                            .toInt()
                }
            },
        )
    }
}
