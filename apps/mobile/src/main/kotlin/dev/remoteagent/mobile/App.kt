package dev.remoteagent.mobile

import android.content.Context
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.ClientSession
import dev.remoteagent.core.ErrorScope
import dev.remoteagent.core.Intent
import dev.remoteagent.core.ListSessions
import dev.remoteagent.core.LoadHostName
import dev.remoteagent.core.OperationKey
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.ThreadList
import dev.remoteagent.core.UploadAttachment
import dev.remoteagent.core.parseInvitation
import dev.remoteagent.core.ticketIdentity
import java.io.File
import java.io.IOException
import kotlin.time.Duration.Companion.milliseconds
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException

internal enum class Screen {
    Hosts,
    Pairing,
    Threads,
    Conversation,
}

/** One core Snapshot feeds Compose; all business changes are queued typed intents. */
// The selected Store owns browser requests as well as conversation operations.
@Suppress("TooManyFunctions")
internal class AndroidAppModel(private val context: Context) : ViewModel() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val repository = AndroidMobileRepository(context)
    var snapshot by mutableStateOf(Snapshot.empty())
        private set

    var conversation by mutableStateOf<ConversationProjection?>(null)
        private set

    var profiles by mutableStateOf(emptyList<HostProfile>())
        private set

    var profileId by mutableStateOf<String?>(null)
        private set

    var screen by mutableStateOf(Screen.Hosts)
    var busy by mutableStateOf(false)
    var notice by mutableStateOf<String?>(null)
    val loadingHistory: Boolean
        get() = snapshot.navigation().threadId?.let { snapshot.operationRunning(OperationKey.History(it)) } ?: false

    val loadingThreads: Boolean
        get() = snapshot.operationRunning(OperationKey.SessionList)

    var list by mutableStateOf<ThreadList?>(null)
        private set

    private var session: ClientSession? = null
    private var owner: AgentStore? = null
    private var connection: Job? = null
    private var attachmentTransfers = 0
    private var observation: Job? = null
    val draftKey
        get() = snapshot.navigation().draftKey

    val selectionKey
        get() = profileId to draftKey

    init {
        try {
            profiles = repository.profiles()
            repository.selected?.takeIf { id -> profiles.any { it.id == id } }?.let(::selectProfile)
        } catch (error: SerializationException) {
            notice = error.message
        }
    }

    suspend fun browser(
        expectedProfile: String?,
        request: dev.remoteagent.core.BrowserRequest,
    ): dev.remoteagent.core.BrowserFrame {
        val store = owner ?: error("Host に接続してください。")
        check(profileId == expectedProfile)
        val frame = store.browser(request)
        if (owner !== store || profileId != expectedProfile) throw CancellationException()
        return frame
    }

    fun perform(intent: Intent, complete: (Result<Outcome>) -> Unit = {}) {
        val store = session
        if (store == null) {
            complete(Result.failure(IllegalStateException("Host not connected")))
            return
        }
        val receipt =
            try {
                store.dispatch(intent)
            } catch (error: AgentException) {
                complete(Result.failure(error))
                return
            }
        publish(store.snapshot())
        val host = profileId
        scope.launch {
            val result = runCatching { receipt.wait() }
            if (host == profileId && session === store) publish(store.snapshot())
            complete(result)
        }
    }

    fun selectProfile(id: String) {
        screen = Screen.Threads
        if (profileId == id && owner != null) {
            connect()
            return
        }
        observation?.cancel()
        connection?.cancel()
        val old = session
        val selected = ClientSession.open(repository.sessionStorage(id), old, true, 0uL)
        session = selected
        owner = null
        profileId = id
        repository.selected = id
        busy = false
        publish(Snapshot.empty())
        observation = scope.launch {
            try {
                val store = selected.ready()
                if (session !== selected || !isActive) return@launch
                owner = store
                publish(store.snapshot())
                observe(store, id)
                connect()
            } catch (error: AgentException) {
                if (session === selected && isActive) notice = error.message
            }
        }
    }

    fun pair(contents: String) {
        connection?.cancel()
        busy = true
        notice = null
        connection = scope.launch {
            try {
                val invitation =
                    parseInvitation(contents, System.currentTimeMillis().milliseconds.inWholeSeconds.toULong())
                val id = ticketIdentity(invitation.endpoint)
                val store =
                    ClientSession.pair(
                        repository.sessionStorage(id),
                        session,
                        invitation.endpoint,
                        invitation.invitation,
                        true,
                        0uL,
                    )
                if (!isActive) {
                    store.shutdown()
                    return@launch
                }
                observation?.cancel()
                session?.shutdown()
                profiles = profiles.filterNot { it.id == id } + HostProfile(id, id, invitation.endpoint)
                repository.saveProfiles(profiles)
                repository.selected = id
                profileId = id
                session = store
                val ready = store.ready()
                owner = ready
                publish(ready.snapshot())
                observe(ready, id)
                perform(Intent.LoadHostName(LoadHostName()))
                screen = Screen.Threads
                busy = false
            } catch (error: AgentException) {
                busy = false
                notice = error.message
            }
        }
    }

    fun connect() {
        val store = session
        val profile = profiles.firstOrNull { it.id == profileId }
        if (owner == null || busy) return
        if (store == null || profile == null) return
        busy = true
        notice = null
        connection = scope.launch {
            try {
                store.resume(profile.ticket)
                if (profileId != profile.id) return@launch
                publish(store.snapshot())
                notice = null
            } catch (error: AgentException) {
                if (session === store) notice = error.message
            } finally {
                if (session === store && profileId == profile.id && connection === currentCoroutineContext()[Job])
                    busy = false
            }
        }
    }

    fun lifecycle(active: Boolean) {
        owner?.setConnectionActive(active)
        if (active) {
            // Returning from a picker keeps the transport underneath its upload.
            if (attachmentTransfers == 0) connect()
        } else persist()
    }

    private fun observe(store: AgentStore, id: String) {
        val initial = snapshot
        observation = scope.launch {
            var previous = initial
            while (isActive) {
                store.nextSnapshot(previous)
                if (profileId != id) return@launch
                val latest = store.snapshot()
                publish(latest)
                previous = latest
            }
        }
    }

    private fun publish(next: Snapshot) {
        val changes = next.changes(snapshot)
        if (!changes.changed) return
        if (changes.reconnected) notice = null
        val name = next.hostName()
        if (name != null && profiles.any { it.id == profileId && it.name != name }) {
            profiles = profiles.map { if (it.id == profileId) it.copy(name = name) else it }
            repository.saveProfiles(profiles)
        }
        if (changes.list) list = next.recentThreadList()
        if (changes.conversation) conversation = projectConversationRows(next, next.conversationSource(), conversation)
        snapshot = next
        session?.persistenceError()?.let { notice = it }
    }

    fun attach(selection: Pair<String?, dev.remoteagent.core.DraftKey>, uri: Uri, complete: () -> Unit) {
        if (selection != selectionKey) {
            complete()
            return
        }
        val key = draftKey
        val directory = snapshot.navigation().cwd
        if (!snapshot.connected()) connect()
        attachmentTransfers++
        var local: File? = null
        fun finish() {
            local?.parentFile?.deleteRecursively()
            attachmentTransfers--
            complete()
        }
        scope.launch {
            var uploading = false
            try {
                connection?.join()
                if (selection != selectionKey) return@launch
                if (!snapshot.connected()) throw IOException("PCに接続できません。接続を確認してもう一度添付してください。")
                notice = null
                val attachment =
                    withContext(Dispatchers.IO) { importAttachment(context, uri).also { local = File(it.path) } }
                if (selection != selectionKey) return@launch
                uploading = true
                perform(Intent.UploadAttachment(UploadAttachment(key, attachment, directory))) { finish() }
            } catch (error: SecurityException) {
                if (selection == selectionKey) notice = error.message
            } catch (error: IOException) {
                if (selection == selectionKey) notice = error.message
            } finally {
                if (!uploading) finish()
            }
        }
    }

    fun persist() {
        val selected = session ?: return
        scope.launch {
            try {
                selected.flush()
            } catch (error: AgentException) {
                if (session === selected) notice = error.message
            }
        }
    }

    override fun onCleared() {
        observation?.cancel()
        connection?.cancel()
        val selected = session
        session = null
        owner = null
        selected?.retire()
        scope.launch {
            try {
                selected?.shutdown()
            } finally {
                scope.cancel()
            }
        }
    }
}

internal fun AndroidAppModel.showThreads() {
    screen = Screen.Threads
    perform(Intent.ShowThreadList)
    perform(Intent.ListSessions(ListSessions(query = snapshot.listQuery())))
}

internal fun AndroidAppModel.showHosts() {
    screen = Screen.Hosts
    perform(Intent.ShowThreadList)
}

internal fun AndroidAppModel.older() {
    if (loadingHistory) return
    val id = snapshot.navigation().threadId ?: return
    perform(Intent.ReadOlder(id))
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun RemoteAgentApp(
    activity: ComponentActivity,
    model: AndroidAppModel,
    requestQrScan: (onContents: (String) -> Unit) -> Unit,
) {
    DisposableEffect(model, activity) {
        val observer = AndroidConnectionLifecycle({ model.lifecycle(true) }, { model.lifecycle(false) })
        activity.lifecycle.addObserver(observer)
        onDispose { activity.lifecycle.removeObserver(observer) }
    }
    val screen = if (model.profiles.isEmpty()) Screen.Pairing else model.screen
    BackHandler(model.screen != Screen.Hosts) {
        if (model.screen == Screen.Conversation) model.showThreads() else model.showHosts()
    }
    AppTheme {
        Scaffold(
            topBar = {
                if (screen in listOf(Screen.Pairing, Screen.Hosts))
                    TopAppBar(title = { Text("bex", fontWeight = androidx.compose.ui.text.font.FontWeight.Bold) })
            }
        ) { padding ->
            Column(Modifier.fillMaxSize().appContentInsets(padding)) {
                ConnectionStatus(
                    model.notice,
                    model.busy,
                    !model.snapshot.connected() && model.profileId != null && model.screen != Screen.Pairing,
                    model::connect,
                )
                ErrorNotices(
                    model.snapshot,
                    ErrorScope.Application,
                    model::perform,
                    model::connect,
                )
                when (screen) {
                    Screen.Pairing -> PairingScreen(model.busy, model::pair, model::showHosts, requestQrScan)
                    Screen.Hosts ->
                        ProfilesScreen(model.profiles, model::selectProfile) { model.screen = Screen.Pairing }
                    Screen.Threads -> {
                        ErrorNotices(model.snapshot, ErrorScope.Projects, model::perform)
                        Box(Modifier.weight(1f)) {
                            ThreadListScreen(
                                model.list,
                                model.snapshot.listQuery(),
                                model.loadingThreads,
                                { intent ->
                                    if (intent is Intent.NewChat || intent is Intent.ReadThread) {
                                        model.screen = Screen.Conversation
                                    }
                                    model.perform(intent)
                                },
                                model::showHosts,
                                hostName = model.profiles.firstOrNull { it.id == model.profileId }?.name ?: "My PC",
                                connected = model.snapshot.connected(),
                            )
                        }
                    }
                    Screen.Conversation -> ConversationPane(model)
                }
            }
        }
    }
}

@Composable
private fun PairingScreen(
    busy: Boolean,
    pair: (String) -> Unit,
    showHosts: () -> Unit,
    scan: (onContents: (String) -> Unit) -> Unit,
) {
    var contents by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("PCとペアリング", style = MaterialTheme.typography.headlineMedium)
        Text("PC Host ManagerのQRコードを読み取ります。QRの内容はこの端末に保存しません。")
        Button(onClick = { scan { value -> contents = value } }) { Text("QRコードを読み取る") }
        OutlinedTextField(
            contents,
            { contents = it },
            Modifier.fillMaxWidth(),
            label = { Text("ペアリングQR（手入力）") },
            minLines = 3,
        )
        Button(onClick = { pair(contents) }, enabled = contents.isNotBlank() && !busy) { Text("ペアリング") }
        Button(onClick = showHosts) { Text("戻る") }
    }
}

@Composable
private fun ProfilesScreen(profiles: List<HostProfile>, select: (String) -> Unit, pair: () -> Unit) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item { Button(onClick = pair) { Text("PCを追加") } }
        items(profiles, key = { it.id }) { profile ->
            Card(onClick = { select(profile.id) }, modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Column(Modifier.padding(16.dp)) {
                    Text(profile.name)
                    Text(profile.id, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

@Composable
private fun ConnectionStatus(notice: String?, busy: Boolean, reconnect: Boolean, connect: () -> Unit) {
    notice?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp)) }
    if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    if (reconnect) Button(onClick = connect, enabled = !busy) { Text("再接続") }
}

@Composable
private fun androidx.compose.foundation.layout.ColumnScope.ConversationPane(model: AndroidAppModel) {
    androidx.compose.runtime.key(model.selectionKey) {
        var scrollToTopRequest by remember { mutableStateOf(0) }
        ConversationHeader(
            model.snapshot.navigation().threadId?.let(model.snapshot::conversation)?.title(),
            model::showThreads,
        ) {
            scrollToTopRequest += 1
        }
        androidx.compose.foundation.layout.Row {
            TerminalLauncher(model.snapshot, model::perform)
            BrowserLauncher(model.snapshot.navigation().threadId, model.profileId, model::browser)
        }
        ThreadDetailScreen(
            model.snapshot,
            model.conversation,
            model::perform,
            if (model.loadingHistory) null else model::older,
            Modifier.weight(1f),
            scrollToTopRequest = scrollToTopRequest,
        ) { onSend ->
            ThreadComposer(model.snapshot, model::perform, onSend) { transferState ->
                AttachmentButton(model.selectionKey, model::attach, transferState)
            }
        }
    }
}

// Consume Scaffold/system-bar padding before applying the remaining keyboard inset.
// Keep IME handling at the screen boundary so it cannot create a gap below the composer.
internal fun Modifier.appContentInsets(padding: PaddingValues): Modifier =
    padding(padding).consumeWindowInsets(padding).imePadding()
