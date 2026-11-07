package dev.remoteagent.mobile

import dev.remoteagent.core.AgentException

/** Core supplies safe error sources; views format them in the current language. */
internal fun Throwable.displayMessage(): String? =
    when (this) {
        is AgentException.Failed -> reason
        else -> message
    }
