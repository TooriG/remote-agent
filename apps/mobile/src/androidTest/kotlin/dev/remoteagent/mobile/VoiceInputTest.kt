package dev.remoteagent.mobile

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent as AndroidIntent
import android.speech.RecognizerIntent
import androidx.activity.compose.LocalActivityResultRegistryOwner
import androidx.activity.result.ActivityResultRegistry
import androidx.activity.result.ActivityResultRegistryOwner
import androidx.activity.result.contract.ActivityResultContract
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.core.app.ActivityOptionsCompat
import dev.remoteagent.core.DraftKey
import dev.remoteagent.core.Intent
import dev.remoteagent.core.Snapshot
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class VoiceInputTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun recognitionRestoresItsTargetAndAppendsUnicodeWithoutSubmitting() {
        val original = draftSnapshot("original", "既存の依頼")
        val otherJson = JSONObject(original.serialize().decodeToString())
        otherJson
            .getJSONObject("navigation")
            .put("draft_key", JSONObject().put("Local", JSONObject().put("key", "other")))
        otherJson.getJSONArray("drafts").getJSONArray(0).getJSONObject(1).put("text", "更新後の依頼")
        val other = Snapshot.restore(otherJson.toString().encodeToByteArray())
        val registry = SpeechRegistry()
        val intents = mutableListOf<Intent>()
        val restoration = StateRestorationTester(compose)
        var snapshot = original
        restoration.setContent {
            CompositionLocalProvider(LocalActivityResultRegistryOwner provides registry) {
                MaterialTheme { VoiceInputButton(snapshot, { intent, _ -> intents.add(intent) }, true) }
            }
        }
        compose.onNodeWithContentDescription("音声入力").performClick()
        compose.runOnIdle {
            assertEquals(RecognizerIntent.ACTION_RECOGNIZE_SPEECH, registry.launched?.action)
            assertEquals(
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
                registry.launched?.getStringExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL),
            )
            snapshot = other
        }
        restoration.emulateSavedInstanceStateRestore()
        compose.runOnIdle { registry.complete(Activity.RESULT_OK, "続きの音声🎤") }
        compose.runOnIdle {
            val edit = intents.single() as Intent.EditComposer
            assertEquals(DraftKey.Local("original"), edit.threadId)
            assertEquals("更新後の依頼\n続きの音声🎤", edit.text)
            assertEquals(edit.text.toByteArray(Charsets.UTF_8).size.toUInt(), edit.cursor)
        }
        original.close()
        other.close()
    }

    @Test
    fun successAppendsAndCancellationPreservesTheDraft() {
        draftSnapshot("original", "既存の依頼").use { snapshot ->
            val registry = SpeechRegistry()
            val intents = mutableListOf<Intent>()
            compose.setContent {
                CompositionLocalProvider(LocalActivityResultRegistryOwner provides registry) {
                    MaterialTheme { VoiceInputButton(snapshot, { intent, _ -> intents.add(intent) }, true) }
                }
            }
            compose.onNodeWithContentDescription("音声入力").performClick()
            compose.runOnIdle { registry.complete(Activity.RESULT_CANCELED) }
            compose.runOnIdle { assertTrue(intents.isEmpty()) }
            compose.onNodeWithContentDescription("音声入力").performClick()
            compose.runOnIdle { registry.complete(Activity.RESULT_OK, "音声の追記") }
            compose.runOnIdle {
                val edit = intents.single() as Intent.EditComposer
                assertEquals("既存の依頼\n音声の追記", edit.text)
            }
        }
    }

    @Test
    fun unavailableRecognizerShowsAnActionableMessageWithoutEditing() {
        Snapshot.empty().use { snapshot ->
            val intents = mutableListOf<Intent>()
            val registry = SpeechRegistry(unavailable = true)
            compose.setContent {
                CompositionLocalProvider(LocalActivityResultRegistryOwner provides registry) {
                    MaterialTheme { VoiceInputButton(snapshot, { intent, _ -> intents.add(intent) }, true) }
                }
            }
            compose.onNodeWithContentDescription("音声入力").performClick()
            compose.onNodeWithText("音声認識アプリが見つかりません。端末の音声入力を設定してください。").assertExists()
            compose.runOnIdle { assertTrue(intents.isEmpty()) }
        }
    }
}

private class SpeechRegistry(private val unavailable: Boolean = false) :
    ActivityResultRegistry(), ActivityResultRegistryOwner {
    override val activityResultRegistry: ActivityResultRegistry
        get() = this

    var launched: AndroidIntent? = null
    private var code = 0

    override fun <I, O> onLaunch(
        requestCode: Int,
        contract: ActivityResultContract<I, O>,
        input: I,
        options: ActivityOptionsCompat?,
    ) {
        if (unavailable) throw ActivityNotFoundException()
        code = requestCode
        launched = input as AndroidIntent
    }

    fun complete(result: Int, text: String? = null) {
        val data = text?.let {
            AndroidIntent().putStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS, arrayListOf(it))
        }
        dispatchResult(code, result, data)
    }
}

private fun draftSnapshot(id: String, text: String): Snapshot {
    val key = JSONObject().put("Local", JSONObject().put("key", id))
    val json = JSONObject(Snapshot.empty().use { it.serialize().decodeToString() })
    json.getJSONObject("navigation").put("draft_key", key)
    json.put(
        "drafts",
        JSONArray().put(JSONArray().put(key).put(JSONObject().put("text", text).put("attachments", JSONArray()))),
    )
    return Snapshot.restore(json.toString().encodeToByteArray())
}
