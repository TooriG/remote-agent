package dev.remoteagent.mobile

import dev.remoteagent.core.AgentException

/** Native reasons are already prepared for display by core. */
internal fun Throwable.displayMessage(): String? =
    when (this) {
        is AgentException.Failed -> reason
        else -> message
    }
