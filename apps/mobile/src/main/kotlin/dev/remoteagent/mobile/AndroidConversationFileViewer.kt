package dev.remoteagent.mobile

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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.Intent
import dev.remoteagent.core.MarkdownFileReference
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.ReadFile
import dev.remoteagent.core.fileHostPath
import dev.remoteagent.core.mobileFileOpenRequest
import kotlinx.coroutines.CancellationException

/** Text reads use the existing Host file operation; unsupported/binary files keep its read error. */
@Composable
internal fun ConversationFileViewer(
    reference: MarkdownFileReference,
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
    source: String? = null,
    workspaceRoot: String? = null,
    dismiss: () -> Unit,
) {
    val snapshot = LocalHostSnapshot.current
    val cwd = workspaceRoot ?: snapshot.navigation().cwd
    val home = snapshot.hostHomeDirectory()
    var path by remember(reference.path) { mutableStateOf(fileHostPath(reference.path, home)) }
    var directory by remember(reference.path) { mutableStateOf(false) }
    var error by remember(reference.path) { mutableStateOf<String?>(null) }
    var loading by remember(reference.path) { mutableStateOf(true) }
    LaunchedEffect(reference.path) {
        try {
            if (source != null) {
                val request = mobileFileOpenRequest(source, cwd, home)
                if (request != null) {
                    val result =
                        fileOperation(Intent.ResolveFileReference(request), perform) as Outcome.FileReferenceResolved
                    path = result.path
                    directory = result.directory
                }
            }
            if (!directory) fileOperation(Intent.ReadFile(ReadFile(path, false)), perform)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: AgentException) {
            error = failure.displayMessage() ?: "ファイルを開けません"
        } catch (failure: IllegalStateException) {
            error = failure.displayMessage() ?: "ファイルを開けません"
        } finally {
            loading = false
        }
    }
    if (directory && perform != null) {
        WorkspaceFilesDialog(snapshot, path, perform, dismiss)
    } else {
        val text = snapshot.file()?.takeIf { it.path == path }?.text
        AlertDialog(
            onDismissRequest = dismiss,
            title = { Text(reference.label) },
            text = {
                if (loading) CircularProgressIndicator() else FileViewerBody(path, text, error, reference.line, perform)
            },
            confirmButton = { TextButton(onClick = dismiss) { Text(LocalUiLanguage.current.text("閉じる")) } },
        )
    }
}

@Composable
private fun FileViewerBody(
    path: String,
    text: String?,
    error: String?,
    line: UInt?,
    perform: ((Intent, (Result<Outcome>) -> Unit) -> Unit)?,
) {
    Column(Modifier.heightIn(max = 480.dp)) {
        SelectionContainer { Text(path) }
        error?.let { Text(it) }
        if (text != null && error == null) {
            if (
                line == null &&
                    dev.remoteagent.core.markdownFileKind(path) == dev.remoteagent.core.MarkdownFileKind.MARKDOWN
            ) {
                PreviewText(text, true, path.replace('\\', '/').substringBeforeLast('/'), perform)
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
