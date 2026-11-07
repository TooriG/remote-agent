package dev.remoteagent.mobile

import android.app.Application
import dev.remoteagent.core.DevicePreferences

class BexApplication : Application() {
    lateinit var devicePreferences: DevicePreferences
        private set

    override fun onCreate() {
        super.onCreate()
        devicePreferences = DevicePreferences(AndroidPreferenceStorage(this))
    }
}
