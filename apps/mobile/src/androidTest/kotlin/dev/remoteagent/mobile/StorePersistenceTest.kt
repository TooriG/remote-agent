package dev.remoteagent.mobile

import android.content.Context
import android.content.ContextWrapper
import android.content.SharedPreferences
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.remoteagent.core.PreferenceKey
import java.io.File
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class StorePersistenceTest {
    @Test
    fun androidStorageKeepsHostsAndDevicePreferencesSeparate() {
        val base = InstrumentationRegistry.getInstrumentation().targetContext
        val token = UUID.randomUUID().toString()
        val directory = File(base.cacheDir, "snapshot-test-$token").apply { mkdirs() }
        val context =
            object : ContextWrapper(base) {
                override fun getFilesDir(): File = directory

                override fun getSharedPreferences(name: String, mode: Int): SharedPreferences =
                    base.getSharedPreferences("$token-$name", mode)
            }
        try {
            val repository = AndroidMobileRepository(context)
            val first = "opaque host A".encodeToByteArray()
            val second = "opaque host B".encodeToByteArray()
            repository.sessionStorage("host-a").save(first)
            repository.sessionStorage("host-b").save(second)
            assertTrue(repository.sessionStorage("host-a").load().contentEquals(first))
            assertTrue(repository.sessionStorage("host-b").load().contentEquals(second))

            val storage = AndroidPreferenceStorage(context)
            for ((key, bytes) in
                listOf(
                    PreferenceKey.MODELS to "opaque model choices".encodeToByteArray(),
                    PreferenceKey.EDITOR to "opaque editor choice".encodeToByteArray(),
                )) {
                storage.save(key, bytes)
            }
            assertEquals("opaque model choices", storage.load(PreferenceKey.MODELS).decodeToString())
            assertEquals("opaque editor choice", storage.load(PreferenceKey.EDITOR).decodeToString())
            assertTrue(repository.sessionStorage("host-a").load().contentEquals(first))

            val language = context.getSharedPreferences("bex-app", Context.MODE_PRIVATE)
            language.edit().putInt("language", 42).commit()
            assertTrue(storage.load(PreferenceKey.LANGUAGE).isEmpty())
            assertEquals(42, language.getInt("language", 0))
            storage.save(PreferenceKey.LANGUAGE, "\"en\"".encodeToByteArray())
            assertEquals("en", language.getString("language", null))
            assertEquals("\"en\"", AndroidPreferenceStorage(context).load(PreferenceKey.LANGUAGE).decodeToString())
        } finally {
            directory.deleteRecursively()
            base.deleteSharedPreferences("$token-agent-hosts")
            base.deleteSharedPreferences("$token-bex-app")
        }
    }
}
