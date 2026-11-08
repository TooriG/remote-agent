package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Outcome
import dev.remoteagent.core.Snapshot
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class UnknownSubmissionTest {
    @get:Rule val compose = createComposeRule()

    private fun fixture(unknown: Boolean = true, count: Int = 1): Snapshot {
        val key = JSONObject("""{"Local":{"key":"new:/fixture"}}""")
        val draft =
            JSONObject(
                """{"text":"過去の指示","attachments":[{"path":"/sent.txt","name":"sent.txt","isImage":false}],"model":null,"effort":null}"""
            )
        val pending =
            JSONObject()
                .put("submitted_at_ms", 1_700_000_000_000L)
                .put("draft_key", key)
                .put("draft", draft)
                .put("accepted", false)
                .put("delivery_unknown", unknown)
        val submissions = JSONObject()
        for (index in 0 until count) {
            val id = if (index == 0) "old-input" else "old-input-$index"
            submissions.put(id, JSONObject(pending.toString()).put("sequence", index))
        }
        val state =
            JSONObject(Snapshot.empty().serialize().decodeToString())
                .put("navigation", JSONObject().put("draft_key", key).put("cwd", "/fixture"))
                .put("pending_submissions", submissions)
        return Snapshot.restore(state.toString().encodeToByteArray())
    }

    @Test
    fun discardActionRemovesTheDisplayedInstructionWhenPublicationUpdates() {
        val current = mutableStateOf(fixture())
        val intents = mutableListOf<Intent>()
        compose.setContent {
            val snapshot = current.value
            MaterialTheme {
                ThreadDetailFixture(
                    snapshot,
                    projectConversationRows(snapshot),
                    { intent, done ->
                        intents += intent
                        done(Result.success(Outcome.Applied))
                    },
                ) {}
            }
        }
        compose.onNodeWithText("過去の指示", substring = true).assertDoesNotExist()
        compose.onNodeWithText("内容を確認").performClick()
        compose.onNodeWithText("過去の指示", substring = true).assertIsDisplayed()
        compose.onNodeWithText("表示を削除").performClick()
        compose.runOnIdle {
            assertEquals(listOf(Intent.DiscardUnknownSubmission("old-input")), intents)
            current.value = fixture(count = 0)
        }
        compose.onNodeWithText("過去の指示", substring = true).assertDoesNotExist()
    }

    @Test
    fun restoreActionDispatchesOnlyTheSelectedInput() {
        fixture().use { snapshot ->
            val item =
                (requireNotNull(projectConversationRows(snapshot)).queuedRows().single().data().content
                        as dev.remoteagent.core.ConversationRowContent.User)
                    .item
            val intents = mutableListOf<Intent>()
            compose.setContent {
                MaterialTheme {
                    ThreadMessageCard(
                        item,
                        true,
                        "/fixture",
                        { intent, done ->
                            intents += intent
                            done(Result.success(Outcome.Applied))
                        },
                    )
                }
            }
            compose.onNodeWithText("下書きに戻す").performClick()
            compose.runOnIdle { assertEquals(listOf(Intent.RestoreUnknownSubmission("old-input")), intents) }
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
    fun sendingInputShowsTimeAndCopyWithoutDeliveryOrRecoveryLabels() {
        fixture(false).use { snapshot ->
            val item =
                (requireNotNull(projectConversationRows(snapshot)).queuedRows().single().data().content
                        as dev.remoteagent.core.ConversationRowContent.User)
                    .item
            compose.setContent { MaterialTheme { ThreadMessageCard(item, true, "/fixture", { _, _ -> }) } }
            val sentTime =
                java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT)
                    .format(java.util.Date(1_700_000_000_000L))
            compose.onNodeWithText(sentTime).assertIsDisplayed()
            compose.onNodeWithContentDescription("コピー").assertIsDisplayed()
            compose.onNodeWithText("送信中…").assertDoesNotExist()
            compose.onNodeWithText("送信済み").assertDoesNotExist()
            compose.onNodeWithText("表示を削除").assertDoesNotExist()
            compose.onNodeWithText("下書きに戻す").assertDoesNotExist()
        }
    }

    @Test
    fun bulkDiscardDispatchesEachDisplayedInput() {
        val current = mutableStateOf(fixture(count = 3))
        val intents = mutableListOf<Intent>()
        compose.setContent {
            val snapshot = current.value
            MaterialTheme {
                ThreadDetailFixture(
                    snapshot,
                    projectConversationRows(snapshot),
                    { intent, done ->
                        intents += intent
                        done(Result.success(Outcome.Applied))
                    },
                ) {}
            }
        }
        compose.onNodeWithText("送信結果不明・3件").assertIsDisplayed()
        compose.onNodeWithText("過去の指示", substring = true).assertDoesNotExist()
        compose.onNodeWithText("表示をすべて削除").performClick()
        compose.waitForIdle()
        compose.runOnIdle {
            assertEquals(
                listOf(
                    Intent.DiscardUnknownSubmission("old-input"),
                    Intent.DiscardUnknownSubmission("old-input-1"),
                    Intent.DiscardUnknownSubmission("old-input-2"),
                ),
                intents,
            )
            current.value = fixture(count = 0)
        }
        compose.onNodeWithText("送信結果不明・3件").assertDoesNotExist()
    }

    @Test
    fun bulkDiscardStopsOnFailureAndRetriesRemainingInputs() {
        val ids = mutableStateOf(listOf("first", "second"))
        val calls = mutableListOf<String>()
        var fail = true
        compose.setContent {
            MaterialTheme {
                UnknownSubmissionSummary(
                    ids.value,
                    false,
                    {},
                    { intent, done ->
                        val id = (intent as Intent.DiscardUnknownSubmission).clientUserMessageId
                        calls += id
                        if (id == "second" && fail) done(Result.failure(IllegalStateException("storage")))
                        else {
                            ids.value = ids.value - id
                            done(Result.success(Outcome.Applied))
                        }
                    },
                )
            }
        }
        compose.onNodeWithText("表示をすべて削除").performClick()
        compose.onNodeWithText("削除できませんでした。残った表示をもう一度削除できます。").assertIsDisplayed()
        compose.runOnIdle { fail = false }
        compose.onNodeWithText("表示をすべて削除").performClick()
        compose.runOnIdle { assertEquals(listOf("first", "second", "second"), calls) }
    }

    @Test
    fun bulkDiscardFreezesBatchAndDisablesRepeatTaps() {
        val ids = mutableStateOf(listOf("first", "second"))
        val calls = mutableListOf<String>()
        var completion: ((Result<Outcome>) -> Unit)? = null
        compose.setContent {
            MaterialTheme {
                UnknownSubmissionSummary(
                    ids.value,
                    false,
                    {},
                    { intent, done ->
                        calls += (intent as Intent.DiscardUnknownSubmission).clientUserMessageId
                        completion = done
                    },
                )
            }
        }
        compose.onNodeWithText("表示をすべて削除").performClick()
        compose.onNodeWithText("削除中…").assertIsNotEnabled()
        compose.onNodeWithText("内容を確認").assertIsNotEnabled()
        compose.runOnIdle {
            ids.value = ids.value + "new-arrival"
            completion!!(Result.success(Outcome.Applied))
        }
        compose.waitForIdle()
        compose.runOnIdle { completion!!(Result.success(Outcome.Applied)) }
        compose.runOnIdle { assertEquals(listOf("first", "second"), calls) }
    }
}
