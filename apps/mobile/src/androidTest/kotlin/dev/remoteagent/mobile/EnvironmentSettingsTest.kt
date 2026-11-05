package dev.remoteagent.mobile

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import dev.remoteagent.core.Presentation
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class EnvironmentSettingsTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun connectionAndProviderDetailsStayOnTheOpenedHost() {
        val changes = mutableListOf<Pair<String, Boolean>>()
        var closed = 0
        compose.setContent {
            MaterialTheme {
                EnvironmentSettingsScreen(
                    HostProfile("server", "Development server", ""),
                    Presentation.empty().settings(),
                    { host, enabled -> changes += host to enabled },
                    { _, _ -> },
                    { closed++ },
                )
            }
        }
        compose.onNodeWithText("Development server").assertIsDisplayed()
        compose.onNodeWithText("Bex Host").assertIsDisplayed()
        compose.onNodeWithTag("environment.enabled").performClick()
        compose.runOnIdle { assertEquals(listOf("server" to false), changes) }
        compose.onNodeWithText("Codex", substring = false).performScrollTo().performClick()
        compose.onNodeWithText("アカウント").assertIsDisplayed()
        compose.onNodeWithText("Codex CLI").performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("戻る").performScrollTo().performClick()
        compose.runOnIdle { assertEquals(0, closed) }
        compose.onNodeWithTag("environment.enabled").assertIsDisplayed()
        compose.onNodeWithText("戻る").performClick()
        compose.runOnIdle { assertEquals(1, closed) }
    }
}
