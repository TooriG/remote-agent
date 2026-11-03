package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.ClientBuild
import dev.remoteagent.core.ClientPlatform
import dev.remoteagent.core.ClientSession
import dev.remoteagent.core.ClientStorage
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Snapshot
import dev.remoteagent.core.StorageKey
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class ModelSelectionTest {
    @get:Rule val compose = createComposeRule()

    private class Storage(snapshot: Snapshot) : ClientStorage, ClientPlatform {
        private val values = mutableMapOf(StorageKey.LOCAL_STATE to snapshot.serializeLocalState())

        override fun load(key: StorageKey): ByteArray = synchronized(values) { values[key] ?: byteArrayOf() }

        override fun save(key: StorageKey, bytes: ByteArray) {
            synchronized(values) { values[key] = bytes }
        }

        override fun identity(): ByteArray = error("offline fixture does not connect")

        override fun diagnosticsDirectory(): String? = null

        override fun conversationDirectory(): String? = null

        fun open(): ClientSession =
            ClientSession.open(this, this, null, false, ClientBuild(0uL, "test-revision"), "Fixture")
    }

    private fun fixture(): Snapshot {
        val key = JSONObject().put("Local", JSONObject().put("key", "new:/original"))
        val state =
            JSONObject(Snapshot.empty().serialize().decodeToString())
                .put("navigation", JSONObject().put("draft_key", key).put("cwd", "/original"))
                .put(
                    "models",
                    JSONArray(
                        """[{"id":"sol","model":{"provider":"codex","id":"sol"},"displayName":"Sol", "settings": {
                        "defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low"},{"reasoningEffort":"medium"}],
                        "serviceTiers":[{"id":"priority","name":"Fast","fast":true}]}}]"""
                    ),
                )
                .put(
                    "drafts",
                    JSONArray()
                        .put(
                            JSONArray()
                                .put(key)
                                .put(
                                    JSONObject(
                                        """{"text":"keep draft","attachments":[],"model":{"provider":"codex","id":"sol"},
                                "effort":"medium","service_tier":"priority"}"""
                                    )
                                )
                        ),
                )
        return Snapshot.restore(state.toString().encodeToByteArray())
    }

    @Test
    fun pickerChoicesSurviveRestartAndSeedNewChats() = runBlocking {
        fixture().use { selected ->
            val storage = Storage(selected)
            val session = storage.open()
            val store = session.ready()
            try {
                compose.setContent {
                    MaterialTheme {
                        ModelPickerButton(
                            selected,
                            { intent, complete ->
                                complete(runBlocking { runCatching { store.dispatch(intent).wait() } })
                            },
                        )
                    }
                }
                compose.onNodeWithTag("model.settings").performClick()
                compose.onNodeWithTag("model.save-defaults").assertDoesNotExist()
                compose.onNodeWithText("低").performScrollTo().performClick()
                session.shutdown()
                val restarted = storage.open()
                try {
                    val restored = restarted.ready()
                    restored.dispatch(Intent.NewChat("/another-project")).wait()
                    val snapshot = restored.snapshot()
                    val draft = snapshot.draft(snapshot.navigation().draftKey)
                    assertEquals("sol", draft.model?.id)
                    assertEquals("low", draft.effort)
                    assertEquals("priority", draft.serviceTier)
                    assertEquals("keep draft", snapshot.draft(selected.navigation().draftKey).text)
                    assertEquals("low", snapshot.draft(selected.navigation().draftKey).effort)
                } finally {
                    restarted.shutdown()
                }
            } finally {
                session.shutdown()
            }
        }
    }

    @Test
    fun openingPickerDoesNotRememberTheDisplayedModel() = runBlocking {
        fixture().use { selected ->
            val store = AgentStore.offline(selected.serializeLocalState(), null, "test-revision")
            try {
                compose.setContent {
                    MaterialTheme {
                        ModelPickerButton(
                            selected,
                            { intent, complete ->
                                complete(runBlocking { runCatching { store.dispatch(intent).wait() } })
                            },
                        )
                    }
                }
                compose.onNodeWithTag("model.settings").performClick()
                compose.onNodeWithText("この設定を新規チャットの既定にする").assertDoesNotExist()
                compose.onNodeWithText("完了").performClick()
                store.dispatch(Intent.NewChat("/another-project")).wait()
                val snapshot = store.snapshot()
                assertEquals(null, snapshot.draft(snapshot.navigation().draftKey).model)
            } finally {
                store.shutdown()
            }
        }
    }
}
