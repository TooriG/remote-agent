package dev.remoteagent.mobile

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import dev.remoteagent.core.EntryKind
import dev.remoteagent.core.FileTreeAction
import dev.remoteagent.core.FileTreeKey
import dev.remoteagent.core.FileTreeRow
import dev.remoteagent.core.FileTreeSurface
import dev.remoteagent.core.FileTreeView
import dev.remoteagent.core.Intent
import dev.remoteagent.core.MarkdownFileKind
import dev.remoteagent.core.MarkdownFileReference
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.fileHostPath
import dev.remoteagent.core.fileTreePath
import java.util.Locale
import java.util.UUID

internal val LocalHostSnapshot = compositionLocalOf { Snapshot.empty() }

@Composable
internal fun FilesLauncher(snapshot: Snapshot, perform: (Intent, (Result<Outcome>) -> Unit) -> Unit) {
    var opened by remember { mutableStateOf(false) }
    TextButton(onClick = { opened = true }) { Text("Files") }
    if (opened) WorkspaceFilesDialog(snapshot, snapshot.navigation().cwd, perform) { opened = false }
}

@Composable
internal fun WorkspaceFilesDialog(
    snapshot: Snapshot,
    directory: String,
    perform: (Intent, (Result<Outcome>) -> Unit) -> Unit,
    dismiss: () -> Unit,
) {
    val owner = remember { UUID.randomUUID().toString() }
    val root = fileHostPath(directory, snapshot.hostHomeDirectory())
    val key = remember(root) { FileTreeKey(owner, root) }
    val tree = snapshot.fileTree(key)
    val currentPerform by rememberUpdatedState(perform)
    val send: (FileTreeAction) -> Unit = { currentPerform(Intent.FileTree(key, it)) {} }
    var selected by remember(key) { mutableStateOf<MarkdownFileReference?>(null) }
    DisposableEffect(key) {
        send(FileTreeAction.Open(FileTreeSurface.MOBILE, Locale.getDefault().toLanguageTag(), root))
        onDispose { currentPerform(Intent.FileTree(key, FileTreeAction.Close)) {} }
    }
    LaunchedEffect(tree.openFile?.id) {
        tree.openFile?.let { request ->
            val path = fileTreePath(root, request.path)
            selected =
                MarkdownFileReference(path, request.path.substringAfterLast('/'), MarkdownFileKind.FILE, null, null)
        }
    }
    Dialog(onDismissRequest = dismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().imePadding()) {
            Column {
                FilesHeader(
                    tree.query,
                    root,
                    { send(FileTreeAction.Query(it)) },
                    { send(FileTreeAction.Refresh) },
                    dismiss,
                )
                FilesTree(tree, send, Modifier.weight(1f))
            }
        }
    }
    selected?.let { reference -> ConversationFileViewer(reference, perform) { selected = null } }
}

@Composable
private fun FilesHeader(
    query: String,
    root: String,
    queryChanged: (String) -> Unit,
    refresh: () -> Unit,
    back: () -> Unit,
) {
    var searchOpen by remember { mutableStateOf(false) }
    val keyboard = LocalSoftwareKeyboardController.current
    val focus = remember { FocusRequester() }
    val searching = searchOpen || query.isNotEmpty()
    val closeSearch = {
        queryChanged("")
        searchOpen = false
        keyboard?.hide()
        Unit
    }
    BackHandler(searching) { closeSearch() }
    val language = LocalUiLanguage.current
    Row(
        Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = if (searching) closeSearch else back) {
            Icon(
                painterResource(R.drawable.file_browser_arrow_left),
                language.text(if (searching) "ファイル検索を閉じる" else "戻る"),
            )
        }
        if (searching) {
            TextField(
                value = query,
                onValueChange = queryChanged,
                placeholder = { Text(language.text("ファイルを検索")) },
                singleLine = true,
                modifier = Modifier.weight(1f).focusRequester(focus),
                trailingIcon = {
                    if (query.isNotEmpty())
                        IconButton(onClick = { queryChanged("") }) {
                            Icon(painterResource(R.drawable.file_browser_x), language.text("検索語を消去"))
                        }
                },
            )
            LaunchedEffect(Unit) { focus.requestFocus() }
        } else {
            Column(Modifier.weight(1f)) {
                Text(language.text("ファイル"), style = MaterialTheme.typography.titleMedium)
                Text(
                    root.replace('\\', '/').trimEnd('/').substringAfterLast('/'),
                    maxLines = 1,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            IconButton(onClick = { searchOpen = true }) {
                Icon(painterResource(R.drawable.file_browser_search), language.text("ファイルを検索"))
            }
            FileOptionsMenu(refresh)
        }
    }
}

@Composable
private fun FileOptionsMenu(refresh: () -> Unit) {
    val language = LocalUiLanguage.current
    var menu by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { menu = true }) {
            Icon(painterResource(R.drawable.file_browser_dots), language.text("ファイルの操作"))
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(
                text = { Text(language.text("ファイル一覧を更新")) },
                onClick = {
                    menu = false
                    refresh()
                },
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FilesTree(tree: FileTreeView, send: (FileTreeAction) -> Unit, modifier: Modifier) {
    val list = rememberLazyListState()
    LaunchedEffect(tree.revealId) {
        val index = tree.rows.indexOfFirst { it.path == tree.focusedPath }
        if (index >= 0) list.animateScrollToItem(index + 1)
    }
    PullToRefreshBox(
        isRefreshing = tree.refreshing,
        onRefresh = { send(FileTreeAction.Refresh) },
        modifier = modifier,
    ) {
        LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(vertical = 8.dp)) {
            item {
                tree.error?.let { ErrorTextView(it, Modifier.padding(16.dp)) }
                if (tree.truncated)
                    Text(LocalUiLanguage.current.text("ほかにも一致する項目があります。検索語を絞り込んでください。"), Modifier.padding(16.dp))
                if (tree.rows.isEmpty()) FilesEmpty(tree) { send(FileTreeAction.Refresh) }
            }
            items(tree.rows, key = { it.path }) { row ->
                FilesRow(row) { send(FileTreeAction.Click(row.path, false, false)) }
            }
        }
    }
}

@Composable
private fun FilesEmpty(tree: FileTreeView, refresh: () -> Unit) {
    val language = LocalUiLanguage.current
    Column(Modifier.padding(16.dp)) {
        when {
            tree.error != null -> {
                Text(language.text("ファイルを表示できません"), fontWeight = FontWeight.Bold)
                TextButton(onClick = refresh, enabled = !tree.pending) { Text(language.text("再試行")) }
            }
            tree.pending -> CircularProgressIndicator()
            else -> {
                Text(language.text("ファイルが見つかりません"), fontWeight = FontWeight.Bold)
                Text(language.text(if (tree.query.isBlank()) "作業スペースは空です。" else "別の語句で検索してください。"))
            }
        }
    }
}

@Composable
private fun FilesRow(row: FileTreeRow, click: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.padding(horizontal = 8.dp)
            .fillMaxWidth()
            .background(
                if (row.selected) colors.secondaryContainer else androidx.compose.ui.graphics.Color.Transparent,
                androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
            )
            .clickable(onClick = click)
            .semantics {
                contentDescription = row.path
                selected = row.selected
            }
            .heightIn(min = 42.dp)
            .padding(start = 8.dp + 18.dp * row.depth.toInt(), end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (row.kind == EntryKind.DIRECTORY) {
            Icon(
                painterResource(
                    if (row.expanded) R.drawable.file_browser_chevron_down else R.drawable.file_browser_chevron_right
                ),
                null,
                Modifier.size(12.dp),
                tint = colors.onSurfaceVariant,
            )
            Icon(
                painterResource(R.drawable.file_browser_folder),
                null,
                Modifier.size(17.dp),
                tint = colors.onSurfaceVariant,
            )
        } else {
            Spacer(Modifier.width(12.dp))
            FileEntryIcon(row.path, Modifier.size(17.dp))
        }
        Text(
            row.label,
            Modifier.weight(1f),
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = if (row.selected) FontWeight.Bold else FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            color =
                if (row.selected) colors.onSecondaryContainer
                else colors.onSurfaceVariant.copy(alpha = if (row.ignored) 0.6f else 1f),
        )
        if (row.kind == EntryKind.DIRECTORY && row.expanded && row.loading)
            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
    }
}
