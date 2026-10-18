package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.remoteagent.core.AgentStore
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class UnknownSubmissionTest {
    @get:Rule val compose = createComposeRule()

    private fun fixture(unknown: Boolean = true): Snapshot {
        val key = JSONObject("""{"Local":{"key":"new:/fixture"}}""")
        val draft =
            JSONObject(
                """{"text":"過去の指示","attachments":[{"path":"/sent.txt","name":"sent.txt","isImage":false}],"model":null,"effort":null}"""
            )
        val pending =
            JSONObject()
                .put("sequence", 0)
                .put("draft_key", key)
                .put("draft", draft)
                .put("accepted", false)
                .put("delivery_unknown", unknown)
        val state =
            JSONObject(Snapshot.empty().serialize().decodeToString())
                .put("navigation", JSONObject().put("draft_key", key).put("cwd", "/fixture"))
                .put("pending_submissions", JSONObject().put("old-input", pending))
        return Snapshot.restore(state.toString().encodeToByteArray())
    }

    @Test
    fun discardRemovesOldInstructionAcrossRestartAndNewChatWithoutSending() = runBlocking {
        val bytes = fixture().use { it.serializeLocalState() }
        val store = AgentStore.offline(bytes, null)
        val current = mutableStateOf(store.snapshot())
        val intents = mutableListOf<Intent>()
        try {
            compose.setContent {
                val snapshot = current.value
                MaterialTheme {
                    ThreadDetailScreen(
                        snapshot,
                        projectConversationRows(snapshot, snapshot.conversationSource(), null),
                        { intent, done ->
                            intents += intent
                            runBlocking { store.dispatch(intent).wait() }
                            current.value = store.snapshot()
                            done(Result.success(Outcome.Applied))
                        },
                        null,
                    ) {}
                }
            }
            compose.onNodeWithText("過去の指示", substring = true).assertIsDisplayed()
            compose.onNodeWithText("表示を削除").performClick()
            compose.onNodeWithText("過去の指示", substring = true).assertDoesNotExist()
            assertTrue(intents.single() is Intent.DiscardUnknownSubmission)
            val restarted = AgentStore.offline(store.snapshot().serializeLocalState(), null)
            try {
                restarted.dispatch(Intent.NewChat("/fixture")).wait()
                assertNull(restarted.snapshot().conversationSource())
                assertEquals(
                    "",
                    restarted.snapshot().draft(restarted.snapshot().navigation().draftKey).text,
                )
            } finally {
                restarted.shutdown()
            }
        } finally {
            store.shutdown()
        }
    }

    @Test
    fun restoreReturnsTextAndAttachmentsToDraftWithoutSending() = runBlocking {
        val store = AgentStore.offline(fixture().use { it.serializeLocalState() }, null)
        try {
            val snapshot = store.snapshot()
            val item =
                requireNotNull(
                        projectConversationRows(snapshot, snapshot.conversationSource(), null)
                    )
                    .queued
                    .single()
            val intents = mutableListOf<Intent>()
            compose.setContent {
                MaterialTheme {
                    ThreadMessageCard(
                        item,
                        true,
                        "/fixture",
                        { intent, done ->
                            intents += intent
                            runBlocking { store.dispatch(intent).wait() }
                            done(Result.success(Outcome.Applied))
                        },
                    )
                }
            }
            compose.onNodeWithText("下書きに戻す").performClick()
            assertTrue(intents.single() is Intent.RestoreUnknownSubmission)
            val next = store.snapshot()
            assertNull(next.conversationSource())
            val draft = next.draft(next.navigation().draftKey)
            assertEquals("過去の指示", draft.text)
            assertEquals("/sent.txt", draft.attachments.single().path)
        } finally {
            store.shutdown()
        }
    }

    @Test
    fun operationPreventsRepeatTapsAndAllowsRetryOnFailure() {
        var calls = 0
        var completion: ((Result<Outcome>) -> Unit)? = null
        compose.setContent {
            MaterialTheme {
                UnknownSubmissionActions("old-input") { _, done ->
                    calls++
                    completion = done
                }
            }
        }
        compose.onNodeWithText("表示を削除").performClick()
        compose.onNodeWithText("表示を削除").assertIsNotEnabled()
        compose.onNodeWithText("下書きに戻す").assertIsNotEnabled()
        compose.runOnIdle { completion!!(Result.failure(IllegalStateException("offline"))) }
        compose.onNodeWithText("操作できませんでした。もう一度お試しください。").assertIsDisplayed()
        compose.onNodeWithText("表示を削除").performClick()
        assertEquals(2, calls)
    }

    @Test
    fun sendingInputHasNoDiscardOrRestoreActions() {
        fixture(false).use { snapshot ->
            val item =
                requireNotNull(
                        projectConversationRows(snapshot, snapshot.conversationSource(), null)
                    )
                    .queued
                    .single()
            compose.setContent {
                MaterialTheme { ThreadMessageCard(item, true, "/fixture", { _, _ -> }) }
            }
            compose.onNodeWithText("送信中…").assertIsDisplayed()
            compose.onNodeWithText("表示を削除").assertDoesNotExist()
            compose.onNodeWithText("下書きに戻す").assertDoesNotExist()
        }
    }
}
