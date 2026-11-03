package dev.remoteagent.mobile

import dev.remoteagent.core.AgentException
import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidErrorsTest {
    @Test
    fun nativeErrorsShowTheReasonWithoutBindingFieldNames() {
        val reason = "ファイルまたはフォルダが見つかりません。"
        assertEquals(reason, AgentException.Failed(reason).displayMessage())
    }

    @Test
    fun platformErrorsKeepTheirMessageAndMissingReasonsUseTheCallersFallback() {
        assertEquals("disk full", IOException("disk full").displayMessage())
        assertNull(Exception().displayMessage())
    }
}
