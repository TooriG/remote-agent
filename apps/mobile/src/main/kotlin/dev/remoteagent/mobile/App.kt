package dev.remoteagent.mobile

import android.content.Context
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
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
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.ClientBuild
import dev.remoteagent.core.ClientSession
import dev.remoteagent.core.ClientSessions
import dev.remoteagent.core.ErrorScope
import dev.remoteagent.core.Intent
import dev.remoteagent.core.ListSessions
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
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
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
    private val appPreferences = context.getSharedPreferences("bex-app", Context.MODE_PRIVATE)
    var language by
        mutableStateOf(
            if (appPreferences.getString("language", "ja") == "en") dev.remoteagent.core.UiLanguage.ENGLISH
            else dev.remoteagent.core.UiLanguage.JAPANESE
        )
        private set

    fun selectLanguage(value: dev.remoteagent.core.UiLanguage) {
        appPreferences
            .edit()
            .putString("language", if (value == dev.remoteagent.core.UiLanguage.ENGLISH) "en" else "ja")
            .apply()
        language = value
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val repository = AndroidMobileRepository(context)
    var snapshot by mutableStateOf(Snapshot.empty())
        private set

    var conversation by mutableStateOf<ConversationProjection?>(null)
        private set

    var profiles by mutableStateOf(emptyList<HostProfile>())
        private set

    var connectionSelection by mutableStateOf(dev.remoteagent.core.ConnectionSelection(null, false))
        private set

    var screen by mutableStateOf(Screen.Threads)

    val busy: Boolean
        get() =
            if (screen == Screen.Pairing) pairingConnection != null
            else connectionSelection.hostId?.let(connections::containsKey) == true

    var notice by mutableStateOf<String?>(null)
    val loadingHistory: Boolean
        get() = snapshot.navigation().threadId?.let { snapshot.operationRunning(OperationKey.History(it)) } ?: false

    val shouldReconnect: Boolean
        get() = !snapshot.connected() && connectionSelection.hostId != null && screen != Screen.Pairing

    val loadingThreads: Boolean
        get() = snapshot.operationRunning(OperationKey.SessionList)

    var list by mutableStateOf<ThreadList?>(null)
        private set

    private val sessions = ClientSessions(true, ClientBuild(0uL, BuildConfig.BUILD_REVISION), repository.deviceName)
    private var session: ClientSession? = null
    private var owner: AgentStore? = null

    private val connections = mutableStateMapOf<String, Job>()
    private var pairingConnection by mutableStateOf<Job?>(null)
    private var attachmentTransfers = 0
    private val observations = mutableMapOf<String, Job>()
    val draftKey
        get() = snapshot.navigation().draftKey

    val selectionKey
        get() = connectionSelection.hostId to draftKey

    init {
        try {
            profiles = repository.profiles()
            val selection =
                dev.remoteagent.core.resolveConnectionSelection(
                    profiles.map { it.id },
                    repository.selected,
                    repository.allHosts,
                )
            connectionSelection = selection
            selection.hostId?.let(::activateProfile)
            profiles.forEach { connect(it) }
        } catch (error: SerializationException) {
            notice = error.displayMessage()
        }
    }

    suspend fun browser(
        expectedProfile: String?,
        request: dev.remoteagent.core.BrowserRequest,
    ): dev.remoteagent.core.BrowserFrame {
        val store = owner ?: error("ホストに接続してください。")
        check(connectionSelection.hostId == expectedProfile)
        val frame = store.browser(request)
        if (owner !== store || connectionSelection.hostId != expectedProfile) throw CancellationException()
        return frame
    }

    fun perform(intent: Intent, complete: (Result<Outcome>) -> Unit = {}) {
        val store = session
        if (store == null) {
            complete(Result.failure(IllegalStateException("ホストに接続していません")))
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
        val host = connectionSelection.hostId
        scope.launch {
            val result = runCatching { receipt.wait() }
            if (host == connectionSelection.hostId && session === store) publish(store.snapshot())
            complete(result)
        }
    }

    fun selectConnection(id: String?) {
        connectionSelection = connectionSelection.copy(all = id == null)
        repository.allHosts = id == null
        id?.let(::activateProfile)
        showThreads()
        updateTaskList()
    }

    private fun activateProfile(id: String) {
        if (session != null && connectionSelection.hostId == id && session === sessions.session(id)) return
        screen = Screen.Threads
        val storage = repository.sessionStorage(id)
        val selected = sessions.open(id, storage, storage)
        session = selected
        owner = selected.store()
        connectionSelection = connectionSelection.copy(hostId = id)
        repository.selected = id
        notice = null
        publish(Snapshot.empty())
        selected.dispatch(Intent.ShowThreadList)
        publish(selected.snapshot())
        observe(id, selected)
    }

    private fun updateTaskList() {
        list = sessions.threadList(if (connectionSelection.all) null else connectionSelection.hostId)
    }

    fun performList(intent: Intent, host: String? = null) {
        if (host != null) {
            performOnHost(intent, host)
            return
        }
        if (intent is Intent.NewChat) {
            screen = Screen.Conversation
            perform(intent)
        } else {
            profiles
                .filter { connectionSelection.all || it.id == connectionSelection.hostId }
                .forEach { performOnHost(intent, it.id) }
        }
    }

    fun performOnHost(intent: Intent, host: String) {
        if (intent is Intent.NewChat || intent is Intent.ReadThread) {
            activateProfile(host)
            screen = Screen.Conversation
            perform(intent)
            return
        }
        val selected = sessions.session(host) ?: return
        try {
            val receipt = selected.dispatch(intent)
            updateTaskList()
            scope.launch {
                try {
                    receipt.wait()
                } catch (error: AgentException) {
                    notice = error.displayMessage()
                }
                updateTaskList()
            }
        } catch (error: AgentException) {
            notice = error.displayMessage()
        }
    }

    fun pair(contents: String) {
        pairingConnection?.cancel()
        notice = null
        pairingConnection =
            scope.launch(start = CoroutineStart.LAZY) {
                try {
                    val invitation =
                        parseInvitation(contents, System.currentTimeMillis().milliseconds.inWholeSeconds.toULong())
                    val id = ticketIdentity(invitation.endpoint)
                    val storage = repository.sessionStorage(id)
                    val store = sessions.pair(storage, storage, invitation)
                    if (!isActive) {
                        store.shutdown()
                        return@launch
                    }
                    val paired =
                        profiles.filterNot { it.id == id } + HostProfile(id, invitation.hostName, invitation.endpoint)
                    repository.saveProfiles(paired)
                    observations.remove(id)?.cancel()
                    connections.remove(id)?.cancel()
                    sessions.adopt(id, store)
                    if (!isActive) return@launch
                    profiles = paired
                    pairingConnection = null
                    selectConnection(id)
                } catch (error: AgentException) {
                    notice = error.displayMessage()
                } finally {
                    if (isActive) pairingConnection = null
                }
            }
        pairingConnection?.start()
    }

    fun connect() {
        val profile = profiles.firstOrNull { it.id == connectionSelection.hostId } ?: return
        if (!connections.containsKey(profile.id)) connect(profile, reconnect = true)
    }

    private fun connect(profile: HostProfile, reconnect: Boolean = false) {
        val storage = repository.sessionStorage(profile.id)
        val store = sessions.open(profile.id, storage, storage)
        observe(profile.id, store)
        if (!reconnect && (store.snapshot().connected() || connections.containsKey(profile.id))) return
        connections.remove(profile.id)?.cancel()
        if (connectionSelection.hostId == profile.id) notice = null
        connections[profile.id] =
            scope.launch(start = CoroutineStart.LAZY) {
                try {
                    store.resume(profile.ticket)
                    if (connectionSelection.hostId == profile.id) {
                        publish(store.snapshot())
                        notice = null
                    }
                } catch (error: AgentException) {
                    if (connectionSelection.hostId == profile.id) notice = error.displayMessage()
                } finally {
                    if (isActive) connections.remove(profile.id)
                }
            }
        connections[profile.id]?.start()
    }

    fun lifecycle(active: Boolean) {
        sessions.recordScene(if (active) 1uL else 3uL)
        if (active) {
            // Returning from a picker keeps the transport underneath its upload.
            if (attachmentTransfers == 0) profiles.forEach { connect(it, reconnect = true) }
        } else persist()
    }

    private fun observe(host: String, selected: ClientSession) {
        if (observations.containsKey(host)) return
        observations[host] = scope.launch {
            try {
                val store = selected.ready()
                if (!isActive) return@launch
                var previous = store.snapshot()
                receiveHost(previous, host, true)
                while (isActive) {
                    store.nextSnapshot(previous)
                    val latest = store.snapshot()
                    receiveHost(latest, host, latest.changes(previous).list)
                    previous = latest
                }
            } catch (error: AgentException) {
                if (session === selected && isActive) notice = error.displayMessage()
            }
        }
    }

    private fun receiveHost(next: Snapshot, host: String, listChanged: Boolean) {
        updateProfile(next, host)
        if (connectionSelection.hostId == host) {
            owner = session?.store()
            publish(next)
        } else if (connectionSelection.all && listChanged) updateTaskList()
    }

    private fun publish(next: Snapshot) {
        val changes = next.changes(snapshot)
        if (!changes.changed) return
        if (changes.reconnected) notice = null
        if (changes.list) updateTaskList()
        if (changes.conversation) conversation = projectConversationRows(next, next.conversationSource(), conversation)
        snapshot = next
        session?.persistenceError()?.let { notice = it }
    }

    private fun updateProfile(next: Snapshot, host: String) {
        // The Host's latest addresses let the next cold connection dial it directly.
        val current = profiles.firstOrNull { it.id == host }
        val refreshed =
            current?.copy(name = next.hostName() ?: current.name, ticket = next.hostTicket() ?: current.ticket)
        if (refreshed != null && refreshed != current) {
            profiles = profiles.map { if (it.id == host) refreshed else it }
            repository.saveProfiles(profiles)
        }
    }

    fun attach(selection: Pair<String?, dev.remoteagent.core.DraftKey>, uri: Uri, complete: () -> Unit) {
        if (selection != selectionKey) {
            complete()
            return
        }
        val key = draftKey
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
                connections[selection.first]?.join()
                if (selection != selectionKey) return@launch
                if (!snapshot.connected()) throw IOException("PCに接続できません。接続を確認してもう一度添付してください。")
                notice = null
                val attachment =
                    withContext(Dispatchers.IO) { importAttachment(context, uri).also { local = File(it.path) } }
                if (selection != selectionKey) return@launch
                uploading = true
                perform(Intent.UploadAttachment(UploadAttachment(key, attachment))) { finish() }
            } catch (error: SecurityException) {
                if (selection == selectionKey) notice = error.displayMessage()
            } catch (error: IOException) {
                if (selection == selectionKey) notice = error.displayMessage()
            } finally {
                if (!uploading) finish()
            }
        }
    }

    fun persist() {
        scope.launch {
            try {
                sessions.flush()
            } catch (error: AgentException) {
                notice = error.displayMessage()
            }
        }
    }

    override fun onCleared() {
        observations.values.forEach { it.cancel() }
        pairingConnection?.cancel()
        connections.values.forEach { it.cancel() }
        connections.clear()
        session = null
        owner = null
        sessions.retire()
        scope.launch {
            try {
                sessions.shutdown()
            } finally {
                scope.cancel()
            }
        }
    }
}

internal fun AndroidAppModel.showThreads() {
    screen = Screen.Threads
    perform(Intent.ShowThreadList)
    performList(Intent.ListSessions(ListSessions(query = snapshot.listQuery())))
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
    var settings by remember { mutableStateOf(false) }
    ObserveConnectionLifecycle(activity, model::lifecycle)
    BackHandler(model.screen != Screen.Threads) { model.showThreads() }
    CompositionLocalProvider(LocalUiLanguage provides model.language, LocalHostSnapshot provides model.snapshot) {
        AppTheme {
            Scaffold(
                topBar = { if (model.screen in listOf(Screen.Pairing, Screen.Hosts)) HostAppBar { settings = true } }
            ) { padding ->
                Column(Modifier.fillMaxSize().appContentInsets(padding)) {
                    ConnectionStatus(model.notice, model.busy, model.shouldReconnect, model::connect)
                    ErrorNotices(model.snapshot, ErrorScope.Application, model::perform, model::connect)
                    when (model.screen) {
                        Screen.Pairing -> PairingScreen(model.busy, model::pair, model::showThreads, requestQrScan)
                        Screen.Hosts ->
                            ProfilesScreen(model.profiles, model::selectConnection) { model.screen = Screen.Pairing }
                        Screen.Threads -> {
                            ErrorNotices(model.snapshot, ErrorScope.Projects, model::perform)
                            ThreadListScreen(
                                model.list,
                                model.snapshot.listQuery(),
                                model.loadingThreads,
                                model::performList,
                                title = {
                                    ConnectionListTitle(
                                        model.profiles,
                                        model.connectionSelection,
                                        model.snapshot.connected(),
                                        select = model::selectConnection,
                                        openSettings = { settings = true },
                                    )
                                },
                                scopeKey =
                                    if (model.connectionSelection.all) "all"
                                    else model.connectionSelection.hostId.orEmpty(),
                                emptyContent =
                                    if (model.profiles.isEmpty()) {
                                        { EmptyConnections(connect = { model.screen = Screen.Pairing }) }
                                    } else null,
                            )
                        }
                        Screen.Conversation -> ConversationPane(model)
                    }
                }
            }
        }
    }
    if (settings)
        LanguageSettingsDialog(
            model.language,
            model::selectLanguage,
            openConnections = {
                settings = false
                model.showHosts()
            },
        ) {
            settings = false
        }
}

@Composable
private fun PairingScreen(
    busy: Boolean,
    pair: (String) -> Unit,
    showHosts: () -> Unit,
    scan: (onContents: (String) -> Unit) -> Unit,
) {
    val language = LocalUiLanguage.current
    var contents by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize().padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(language.text("PCとペアリング"), style = MaterialTheme.typography.headlineMedium)
        Text(language.text("PCのBexに表示されたQRコードを読み取ります。QRの内容はこの端末に保存しません。"))
        Button(onClick = { scan { value -> contents = value } }) { Text(language.text("QRコードを読み取る")) }
        OutlinedTextField(
            contents,
            { contents = it },
            Modifier.fillMaxWidth(),
            label = { Text(language.text("ペアリングQR（手入力）")) },
            minLines = 3,
        )
        Button(onClick = { pair(contents) }, enabled = contents.isNotBlank() && !busy) { Text(language.text("ペアリング")) }
        Button(onClick = showHosts) { Text(language.text("戻る")) }
    }
}

@Composable
private fun ProfilesScreen(profiles: List<HostProfile>, select: (String) -> Unit, pair: () -> Unit) {
    val language = LocalUiLanguage.current
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(16.dp)) {
        item { Button(onClick = pair) { Text(language.text("PCを追加")) } }
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
    val language = LocalUiLanguage.current
    notice?.let { Text(language.text(it), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp)) }
    if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
    if (reconnect) Button(onClick = connect, enabled = !busy) { Text(language.text("再接続")) }
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
            FilesLauncher(model.snapshot, model::perform)
            TerminalLauncher(model.snapshot, model::perform)
            BrowserLauncher(model.snapshot.navigation().threadId, model.connectionSelection.hostId, model::browser)
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

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HostAppBar(openSettings: () -> Unit) {
    val language = LocalUiLanguage.current
    TopAppBar(
        title = {
            androidx.compose.foundation.Image(
                androidx.compose.ui.res.painterResource(R.drawable.agent_icon),
                "Bex",
                Modifier.size(26.dp),
            )
        },
        actions = { androidx.compose.material3.TextButton(onClick = openSettings) { Text(language.text("設定")) } },
    )
}
