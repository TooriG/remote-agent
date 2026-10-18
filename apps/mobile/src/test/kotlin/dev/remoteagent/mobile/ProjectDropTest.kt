package dev.remoteagent.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ProjectDropTest {
    private val headers = listOf("a" to (20..99), "b" to (300..379), "c" to (380..479))

    @Test fun choosesInsertionSideAndSkipsExpandedChats() {
        assertEquals(ProjectDrop("a", false), projectDropAt(0f, headers))
        assertEquals(ProjectDrop("a", true), projectDropAt(100f, headers))
        assertEquals(ProjectDrop("b", false), projectDropAt(290f, headers))
        assertEquals(ProjectDrop("b", true), projectDropAt(360f, headers))
        assertEquals(ProjectDrop("c", true), projectDropAt(900f, headers))
        assertNull(projectDropAt(10f, emptyList()))
    }
}
