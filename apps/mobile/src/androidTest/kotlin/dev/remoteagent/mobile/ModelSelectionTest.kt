package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Snapshot
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class ModelSelectionTest {
    @get:Rule val compose = createComposeRule()

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
            val store = AgentStore.offline(selected.serializeLocalState(), null)
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
                val restored = AgentStore.offline(store.snapshot().serializeLocalState(), null)
                try {
                    restored.dispatch(Intent.NewChat("/another-project")).wait()
                    val snapshot = restored.snapshot()
                    val draft = snapshot.draft(snapshot.navigation().draftKey)
                    assertEquals("sol", draft.model?.id)
                    assertEquals("low", draft.effort)
                    assertEquals("priority", draft.serviceTier)
                    assertEquals("keep draft", snapshot.draft(selected.navigation().draftKey).text)
                    assertEquals("low", snapshot.draft(selected.navigation().draftKey).effort)
                } finally {
                    restored.shutdown()
                }
            } finally {
                store.shutdown()
            }
        }
    }

    @Test
    fun openingPickerDoesNotRememberTheDisplayedModel() = runBlocking {
        fixture().use { selected ->
            val store = AgentStore.offline(selected.serializeLocalState(), null)
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
