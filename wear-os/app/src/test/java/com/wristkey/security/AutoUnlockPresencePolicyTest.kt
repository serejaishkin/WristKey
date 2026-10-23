package com.wristkey.security

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AutoUnlockPresencePolicyTest {
    @Test
    fun automaticallyApprovesOnlyWhenSecureWatchIsUnlocked() {
        assertTrue(AutoUnlockPresencePolicy.canAutomaticallyApprove(true, false, false))
        assertFalse(AutoUnlockPresencePolicy.canAutomaticallyApprove(true, true, true))
        assertTrue(AutoUnlockPresencePolicy.canAutomaticallyApprove(false, false, true))
        assertFalse(AutoUnlockPresencePolicy.canAutomaticallyApprove(false, false, false))
    }
}
