package com.wristkey

import java.util.*

/**
 * Represents a paired PC/device.
 */
data class PairedDevice(
    val deviceIdHex: String,
    var name: String,
    val pairedAt: Long = System.currentTimeMillis(),
    var lastConnectedAt: Long? = null,
    var isTrusted: Boolean = true
) {
    fun formattedDate(): String {
        val date = Date(pairedAt)
        return android.text.format.DateFormat.format("yyyy-MM-dd HH:mm", date).toString()
    }
}
