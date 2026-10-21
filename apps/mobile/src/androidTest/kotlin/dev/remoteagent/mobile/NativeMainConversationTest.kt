package dev.remoteagent.mobile

import android.graphics.Bitmap
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.test.platform.app.InstrumentationRegistry
import dev.remoteagent.core.ProviderKind
import dev.remoteagent.core.SessionRef
import dev.remoteagent.core.Snapshot
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Rule
import org.junit.Test

class NativeMainConversationTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun cloudMainConversationUsesTheStandardBexHistoryAndComposer() {
        val identity = JSONObject("""{"provider":"claude","id":"chan_fixture"}""")
        val turns = JSONArray()
            .put(JSONObject().put("id", "question").put("status", "completed")
                .put("items", JSONArray().put(messageItem("question", "今の進捗は？", user = true))))
            .put(JSONObject().put("id", "answer").put("status", "completed")
                .put("items", JSONArray().put(messageItem("answer", "## 進捗\n\nメイン会話をBEXの会話画面で表示しています。"))))
        val persisted = JSONObject(Snapshot.empty().serialize().decodeToString())
            .put("navigation", JSONObject().put("cwd", "").put("thread_id", identity)
                .put("draft_key", JSONObject().put("Session", JSONObject().put("session", identity))))
            .put("conversations", JSONArray().put(JSONArray().put(identity)
                .put(JSONObject().put("id", identity).put("name", "メイン会話 · BEX")
                    .put("canAcceptDirectInput", true).put("status", "idle").put("turns", turns))))
        Snapshot.restore(persisted.toString().encodeToByteArray()).use { snapshot ->
            val session = SessionRef(ProviderKind.CLAUDE, "chan_fixture")
            val projection = projectConversationRows(snapshot, snapshot.conversation(session), null)
            compose.setContent {
                BexTheme {
                    Column {
                        ConversationHeader("メイン会話 · BEX", {}) {}
                        ThreadDetailScreen(snapshot, projection, { _, _ -> }, null) {
                            ComposerTextInput(snapshot) { _, _ -> }
                        }
                    }
                }
            }
            compose.onNodeWithText("今の進捗は？").assertIsDisplayed()
            compose.onNodeWithText("メイン会話をBEXの会話画面で表示しています。").assertIsDisplayed()
            compose.onNodeWithText("AIに依頼する…").assertIsDisplayed()
            val instrumentation = InstrumentationRegistry.getInstrumentation()
            val screenshot = instrumentation.uiAutomation.takeScreenshot()
            File(instrumentation.targetContext.getExternalFilesDir(null), "native-main-conversation.png")
                .outputStream().use { screenshot.compress(Bitmap.CompressFormat.PNG, 100, it) }
            screenshot.recycle()
        }
    }
}
