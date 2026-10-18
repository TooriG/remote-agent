package dev.remoteagent.mobile

import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Scaffold
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.performClick
import androidx.core.view.WindowInsetsCompat
import dev.remoteagent.core.Snapshot
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class ComposerKeyboardTest {
    @get:Rule val compose = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun composerStaysImmediatelyAboveKeyboardAndReturnsToBottomOnDismiss() {
        compose.activityRule.scenario.onActivity {
            it.enableEdgeToEdge(
                statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
                navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            )
            it.window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        }
        Snapshot.empty().use { snapshot ->
            compose.setContent {
                BexTheme {
                    Scaffold { padding ->
                        Column(Modifier.fillMaxSize().appContentInsets(padding)) {
                            Box(Modifier.weight(1f))
                            ThreadComposer(snapshot, { _, _ -> }, {}, {})
                        }
                    }
                }
            }
            val view = compose.activity.window.decorView
            val density = compose.activity.resources.displayMetrics.density
            val originalBottom = compose.onNodeWithTag("conversation.composer").fetchSemanticsNode().boundsInRoot.bottom
            compose.onNode(hasSetTextAction()).performClick()
            compose.waitUntil(10_000) {
                view.rootWindowInsets?.let {
                    WindowInsetsCompat.toWindowInsetsCompat(it, view).isVisible(WindowInsetsCompat.Type.ime())
                } == true
            }
            // Wait for inset animations: the surface must end within its normal 8dp margin of the IME.
            compose.waitUntil(10_000) {
                val ime = WindowInsetsCompat.toWindowInsetsCompat(view.rootWindowInsets, view)
                    .getInsets(WindowInsetsCompat.Type.ime()).bottom
                val bottom = compose.onNodeWithTag("conversation.composer").fetchSemanticsNode().boundsInRoot.bottom
                val gap = view.height - ime - bottom
                ime > 0 && gap >= -density && gap <= 12 * density
            }
            compose.runOnIdle {
                val manager = compose.activity.getSystemService(android.content.Context.INPUT_METHOD_SERVICE)
                    as android.view.inputmethod.InputMethodManager
                manager.hideSoftInputFromWindow(view.windowToken, 0)
            }
            compose.waitUntil(10_000) {
                !WindowInsetsCompat.toWindowInsetsCompat(view.rootWindowInsets, view)
                    .isVisible(WindowInsetsCompat.Type.ime()) &&
                    kotlin.math.abs(compose.onNodeWithTag("conversation.composer").fetchSemanticsNode().boundsInRoot.bottom - originalBottom) < density
            }
            assertTrue(originalBottom > view.height / 2f)
        }
    }
}
