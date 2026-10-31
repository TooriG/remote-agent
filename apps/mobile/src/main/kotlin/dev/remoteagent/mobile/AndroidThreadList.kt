package dev.remoteagent.mobile

import android.graphics.BitmapFactory
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.remoteagent.core.Intent
import dev.remoteagent.core.ListQuery
import dev.remoteagent.core.ListSessions
import dev.remoteagent.core.ProjectSummary
import dev.remoteagent.core.ReadThread
import dev.remoteagent.core.SessionRef
import dev.remoteagent.core.ThreadList
import dev.remoteagent.core.ThreadSummary
import dev.remoteagent.core.WorktreeStatus
import dev.remoteagent.core.localizedUiFormat
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.drop

@Composable
internal fun ThreadListScreen(
    list: ThreadList?,
    query: ListQuery,
    loadingThreads: Boolean,
    perform: (Intent) -> Unit,
    showHosts: () -> Unit,
    hostName: String = "My PC",
    connected: Boolean = false,
    openSettings: () -> Unit = {},
) {
    val language = LocalUiLanguage.current
    var search by rememberSaveable { mutableStateOf(query.searchTerm.takeIf { it.isNotEmpty() }) }
    val focusManager = LocalFocusManager.current
    fun closeSearch() {
        search = null
        focusManager.clearFocus()
        perform(Intent.ListSessions(ListSessions(query = query.copy(searchTerm = ""))))
    }
    BackHandler(enabled = search != null) { closeSearch() }
    var selectedTab by rememberSaveable { mutableStateOf(0) }
    val currentQuery by rememberUpdatedState(query)
    val dispatch by rememberUpdatedState(perform)
    val listState = rememberLazyListState()
    val reorder = rememberProjectReorder(listState, list?.projects.orEmpty(), perform)
    LaunchedEffect(Unit) {
        // Core loads the initial query; every later edit supersedes pending input.
        snapshotFlow { search }
            .drop(1)
            .collectLatest { term ->
                if (term != null) {
                    kotlinx.coroutines.delay(SEARCH_DEBOUNCE_MILLIS)
                    dispatch(Intent.ListSessions(ListSessions(query = currentQuery.copy(searchTerm = term))))
                }
            }
    }
    Column(Modifier.fillMaxSize().padding(horizontal = 20.dp)) {
        ThreadListHeader(
            search = search,
            loadingThreads = loadingThreads,
            changeSearch = { if (it == null) closeSearch() else search = it },
            refresh = { perform(Intent.ListSessions(ListSessions(query = query))) },
        ) {
            Text(
                "bex",
                Modifier.padding(end = 8.dp),
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
            )
            HostSelector(hostName, connected, showHosts, Modifier.weight(1f))
            IconButton(onClick = openSettings) { Icon(painterResource(R.drawable.ic_globe), language.text("設定")) }
        }
        TabRow(selectedTabIndex = selectedTab, containerColor = MaterialTheme.colorScheme.background) {
            listOf(language.text("最近のチャット"), language.text("プロジェクト")).forEachIndexed { index, label ->
                Tab(selected = selectedTab == index, onClick = { selectedTab = index }, text = { Text(label) })
            }
        }
        ThreadListContent(list, loadingThreads, selectedTab, reorder, perform, Modifier.weight(1f).fillMaxWidth())
        NewChatButton(perform)
    }
}

@Composable
private fun NewChatButton(perform: (Intent) -> Unit) {
    val language = LocalUiLanguage.current
    Button(
        onClick = { perform(Intent.NewChat("")) },
        modifier = Modifier.fillMaxWidth().padding(vertical = 12.dp),
        shape = RoundedCornerShape(10.dp),
        contentPadding = PaddingValues(16.dp),
    ) {
        Icon(painterResource(R.drawable.ic_add), null, Modifier.size(20.dp))
        Text(language.text("新しいチャット"), Modifier.padding(start = 8.dp), fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun ThreadListContent(
    list: ThreadList?,
    loadingThreads: Boolean,
    selectedTab: Int,
    reorder: ProjectReorder,
    perform: (Intent) -> Unit,
    modifier: Modifier = Modifier,
) {
    val language = LocalUiLanguage.current
    Box(modifier.clipToBounds()) {
        LazyColumn(Modifier.fillMaxSize(), state = reorder.list, contentPadding = PaddingValues(vertical = 16.dp)) {
            list?.notice?.let { notice -> item { Text(notice, color = MaterialTheme.colorScheme.onSurfaceVariant) } }
            if (selectedTab == 0) {
                recentChats(list, loadingThreads, perform, language)
            } else {
                projectThreads(list, loadingThreads, perform, reorder, language)
            }
        }
        reorder.projects
            .firstOrNull { it.id == reorder.source }
            ?.let { project ->
                // Draw above the list while the original handle stays put to keep gesture coordinates stable.
                ProjectRow(
                    project,
                    {},
                    reorder,
                    floating = true,
                    modifier =
                        Modifier.graphicsLayer { translationY = reorder.draggedTop }
                            .padding(top = 2.dp)
                            .shadow(12.dp, RoundedCornerShape(12.dp))
                            .background(MaterialTheme.colorScheme.surfaceContainerHigh, RoundedCornerShape(12.dp))
                            .border(
                                1.dp,
                                MaterialTheme.colorScheme.primary.copy(alpha = 0.5f),
                                RoundedCornerShape(12.dp),
                            )
                            .clearAndSetSemantics { testTag = "dragged-project" },
                )
            }
    }
}

private fun LazyListScope.recentChats(
    list: ThreadList?,
    loadingThreads: Boolean,
    perform: (Intent) -> Unit,
    language: dev.remoteagent.core.UiLanguage,
) {
    val threads = list?.threads.orEmpty()
    items(threads, key = { "chat:${it.id.listKey}" }) {
        SummaryRow(it, perform, list?.projects?.firstOrNull { project -> project.id == it.projectId }?.name)
    }
    if (list?.hasMore == true)
        item {
            TextButton(
                onClick = { perform(Intent.ExpandRecentChats) },
                enabled = !loadingThreads && !list.loadingChats,
            ) {
                LoadingLabel("もっと見る", list.loadingChats)
            }
        }
    if (list != null && threads.isEmpty())
        item {
            Text(
                language.text("チャットがありません。"),
                Modifier.padding(vertical = 24.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
}

@Composable
private fun HostSelector(hostName: String, connected: Boolean, showHosts: () -> Unit, modifier: Modifier = Modifier) {
    val language = LocalUiLanguage.current
    Row(
        modifier
            .heightIn(min = 48.dp)
            .clickable(
                role = androidx.compose.ui.semantics.Role.Button,
                onClickLabel = language.text("接続先を切り替える"),
                onClick = showHosts,
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_circle),
            if (connected) language.text("接続中") else language.text("未接続"),
            Modifier.size(6.dp),
            tint = if (connected) MaterialTheme.colorScheme.tertiary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Icon(
            painterResource(R.drawable.ic_computer),
            null,
            Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            hostName,
            Modifier.weight(1f, fill = false),
            style = MaterialTheme.typography.labelLarge,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun LoadingLabel(label: String, loading: Boolean) {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        if (loading) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        Text(LocalUiLanguage.current.text(label))
    }
}

@Composable
private fun SummaryRow(thread: ThreadSummary, openConversation: (Intent) -> Unit, projectName: String? = null) {
    val language = LocalUiLanguage.current
    Row(
        Modifier.fillMaxWidth()
            .clickable { openConversation(Intent.ReadThread(ReadThread(thread.id, open = true))) }
            .padding(vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_chat),
            null,
            Modifier.size(22.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Column(Modifier.weight(1f)) {
            Text(thread.title, maxLines = 2, overflow = TextOverflow.Ellipsis)
            projectName?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (thread.active) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        else if (thread.unread) Text(language.text("● 完了・未確認"))
        thread.worktreeStatus?.let { status ->
            val unmerged = status == WorktreeStatus.UNMERGED
            Icon(
                painterResource(if (unmerged) R.drawable.ic_diff else R.drawable.ic_merge),
                if (unmerged) language.text("main に未反映の変更あり") else language.text("main にマージ済み"),
                Modifier.padding(start = 8.dp).size(18.dp),
                tint = if (unmerged) Color(UNMERGED_COLOR_ARGB) else MaterialTheme.colorScheme.tertiary,
            )
        }
        Icon(
            painterResource(R.drawable.ic_chevron),
            null,
            Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
}

private const val DRAG_PLACEHOLDER_ALPHA = 0.2f
private const val SEARCH_DEBOUNCE_MILLIS = 200L
private const val UNMERGED_COLOR_ARGB = 0xFFFB923C

private val SessionRef.listKey: String
    get() = "session:$provider:$id"

private fun LazyListScope.projectThreads(
    list: ThreadList?,
    loadingThreads: Boolean,
    perform: (Intent) -> Unit,
    reorder: ProjectReorder,
    language: dev.remoteagent.core.UiLanguage,
) {
    list?.projects.orEmpty().forEach { project ->
        item(key = "project:${project.id}") {
            ProjectDropMarker(reorder, project.id, after = false)
            ProjectRow(
                project,
                perform,
                reorder,
                modifier = Modifier.alpha(if (reorder.source == project.id) DRAG_PLACEHOLDER_ALPHA else 1f),
            )
            ProjectDropMarker(reorder, project.id, after = true)
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
        }
        if (project.expanded) {
            project.coordinatorUrl?.let {
                item(key = "coordinator:${project.id}") {
                    Text(
                        language.text("タスクの依頼・振り分けはメイン会話へ。以下はこのPCの作業スレッドです。"),
                        Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (project.loading && !project.hasMore)
                item(key = "loading:${project.id}") { CircularProgressIndicator(Modifier.size(18.dp)) }
            project.error?.let { message ->
                item(key = "error:${project.id}") {
                    Text(message)
                    TextButton(onClick = { perform(Intent.RefreshProject(project.id)) }) { Text(language.text("再試行")) }
                }
            }
            items(project.threads, key = { "project:${project.id}:${it.id.listKey}" }) { SummaryRow(it, perform) }
            if (project.hasMore)
                item(key = "more:${project.id}") {
                    TextButton(onClick = { perform(Intent.ExpandThreadList(project.id)) }, enabled = !project.loading) {
                        LoadingLabel("もっと見る", project.loading)
                    }
                }
        }
    }
    if (list?.hasMoreProjects == true)
        item {
            TextButton(
                onClick = { perform(Intent.ExpandProjects) },
                enabled = !loadingThreads && !list.loadingProjects,
            ) {
                LoadingLabel("もっとプロジェクトを表示", list.loadingProjects)
            }
        }
    if (list != null && list.projects.isEmpty())
        item {
            Text(
                language.text("プロジェクトがありません。"),
                Modifier.padding(vertical = 24.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
}

@Composable
private fun ProjectRow(
    project: ProjectSummary,
    perform: (Intent) -> Unit,
    reorder: ProjectReorder,
    modifier: Modifier = Modifier,
    floating: Boolean = false,
) {
    val language = LocalUiLanguage.current
    val openLink = rememberExternalLinkOpener()
    val coordinatorUrl = project.coordinatorUrl
    Row(
        modifier.fillMaxWidth().padding(vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (floating) {
            Box(Modifier.size(40.dp, 48.dp), contentAlignment = Alignment.Center) {
                Text("≡", color = MaterialTheme.colorScheme.primary)
            }
        } else {
            ProjectDragHandle(project, reorder)
        }
        Row(
            Modifier.weight(1f).heightIn(min = 48.dp).clickable {
                perform(Intent.SetProjectExpanded(project.id, !project.expanded))
            },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ProjectIcon(project.iconPng, project.monogram, project.iconColor)
            Column(Modifier.weight(1f)) {
                Text(
                    project.name,
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                project.roots.firstOrNull()?.path?.let { path ->
                    Text(
                        path,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        if (coordinatorUrl != null) {
            IconButton(onClick = { openLink(coordinatorUrl) }) {
                Icon(
                    painterResource(R.drawable.ic_chat),
                    localizedUiFormat(language, "{0}のメイン会話をClaudeで開く", listOf(project.name)),
                )
            }
        } else {
            IconButton(onClick = { perform(Intent.NewChat(project.roots.firstOrNull()?.path.orEmpty())) }) {
                Icon(
                    painterResource(R.drawable.ic_add),
                    localizedUiFormat(language, "{0}で新しいチャット", listOf(project.name)),
                )
            }
        }
    }
}

@Composable
private fun ProjectIcon(png: ByteArray?, monogram: String, colorRgb: UInt) {
    val bitmap = remember(png) { png?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() } }
    val color = Color(colorRgb.toInt()).copy(alpha = 1f)
    val modifier = Modifier.size(34.dp).clip(RoundedCornerShape(8.dp)).clearAndSetSemantics {}
    if (bitmap != null) {
        Image(bitmap, contentDescription = null, modifier = modifier)
    } else {
        Box(modifier.background(color.copy(alpha = 0.15f)), contentAlignment = Alignment.Center) {
            Text(monogram, color = color, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        }
    }
}
