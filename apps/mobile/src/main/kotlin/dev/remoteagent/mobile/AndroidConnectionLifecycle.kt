package dev.remoteagent.mobile

import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner

@Composable
internal fun ObserveConnectionLifecycle(activity: ComponentActivity, sceneChanged: (Boolean) -> Unit) {
    DisposableEffect(activity, sceneChanged) {
        val observer =
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = sceneChanged(true)

                override fun onStop(owner: LifecycleOwner) = sceneChanged(false)
            }
        activity.lifecycle.addObserver(observer)
        onDispose { activity.lifecycle.removeObserver(observer) }
    }
}
