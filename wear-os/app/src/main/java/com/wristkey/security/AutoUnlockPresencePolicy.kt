package com.wristkey.security

object AutoUnlockPresencePolicy {
    fun canAutomaticallyApprove(
        isDeviceSecure: Boolean,
        isDeviceLocked: Boolean,
        hasRecentMotion: Boolean
    ): Boolean {
        return if (isDeviceSecure) !isDeviceLocked else hasRecentMotion
    }
}
