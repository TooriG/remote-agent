package dev.remoteagent.mobile

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.util.AtomicFile
import dev.remoteagent.core.AgentException
import dev.remoteagent.core.Attachment
import dev.remoteagent.core.ClientStorage
import dev.remoteagent.core.generateIdentity
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.util.Base64
import java.util.UUID
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

@Serializable internal data class HostProfile(val id: String, val name: String, val ticket: String)

/** Platform persistence owns profile metadata and opaque core snapshots. */
internal class AndroidMobileRepository(private val context: Context) {
    private val directory = context.filesDir
    private val preferences = context.getSharedPreferences("agent-hosts", Context.MODE_PRIVATE)
    var selected: String?
        get() = preferences.getString("selected", null)
        set(value) {
            preferences.edit().putString("selected", value).apply()
        }

    fun profiles(): List<HostProfile> =
        preferences.getString("profiles", null)?.let { Json.decodeFromString(it) } ?: emptyList()

    fun saveProfiles(profiles: List<HostProfile>) {
        preferences.edit().putString("profiles", Json.encodeToString(profiles)).apply()
    }

    fun diagnosticsDirectory(id: String): String =
        File(
                directory,
                "connection-diagnostics/${encodedId(id)}",
            )
            .absolutePath

    fun load(id: String): ByteArray {
        val file = snapshotFile(id)
        return if (file.baseFile.exists()) file.openRead().use { it.readBytes() } else byteArrayOf()
    }

    fun save(id: String, bytes: ByteArray) = snapshotFile(id).writeSynced(bytes)

    fun sessionStorage(id: String): ClientStorage =
        object : ClientStorage {
            private fun <T> access(body: () -> T): T =
                try {
                    body()
                } catch (error: IOException) {
                    throw AgentException.Failed(error.message ?: error.toString()).apply { initCause(error) }
                } catch (error: GeneralSecurityException) {
                    throw AgentException.Failed(error.message ?: error.toString()).apply { initCause(error) }
                } catch (error: IllegalArgumentException) {
                    throw AgentException.Failed(error.message ?: error.toString()).apply { initCause(error) }
                }

            override fun load(newPairing: Boolean): ByteArray = access {
                if (newPairing) byteArrayOf() else this@AndroidMobileRepository.load(id)
            }

            override fun save(localState: ByteArray, modelPreferences: ByteArray) = access {
                this@AndroidMobileRepository.save(id, localState)
            }

            override fun identity(): ByteArray = access {
                AndroidCredentialStore(context, id).loadOrCreate(::generateIdentity)
            }

            override fun diagnosticsDirectory(): String? = access {
                this@AndroidMobileRepository.diagnosticsDirectory(id)
            }
        }

    private fun snapshotFile(id: String) = AtomicFile(File(directory, "agent-snapshot-${encodedId(id)}.json"))

    private fun encodedId(id: String): String =
        Base64.getUrlEncoder().withoutPadding().encodeToString(id.encodeToByteArray())
}

/** Replaces the file only after the new bytes reach storage. */
internal fun AtomicFile.writeSynced(bytes: ByteArray) {
    val output = startWrite()
    var committed = false
    try {
        output.write(bytes)
        output.fd.sync()
        finishWrite(output)
        committed = true
    } finally {
        if (!committed) failWrite(output)
    }
}

internal fun importAttachment(context: Context, uri: Uri): Attachment {
    val resolver = context.contentResolver
    val name =
        resolver
            .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { if (it.moveToFirst()) it.getString(0) else null }
            ?.substringAfterLast('/') ?: "attachment"
    val folder = File(context.cacheDir, UUID.randomUUID().toString())
    if (!folder.mkdir()) throw IOException("添付用フォルダを作成できません")
    var imported = false
    try {
        val file = File(folder, name)
        resolver.openInputStream(uri)?.use { input -> file.outputStream().use(input::copyTo) }
            ?: throw IOException("添付ファイルを開けません")
        val attachment = Attachment(file.path, name, resolver.getType(uri)?.startsWith("image/") == true)
        imported = true
        return attachment
    } finally {
        if (!imported) folder.deleteRecursively()
    }
}
