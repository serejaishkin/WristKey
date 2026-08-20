package com.wristkey.ble

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BleReconnectPolicyTest {
    @Test fun knownAddressDoesNotNeedPairing() {
        val policy = BleReconnectPolicy { "AA:BB:CC:DD:EE:FF" }
        assertTrue(policy.isKnownDevice("aa:bb:cc:dd:ee:ff"))
        assertFalse(policy.shouldShowPairingUi("AA:BB:CC:DD:EE:FF"))
        assertTrue(policy.shouldAcceptAsReconnect("AA:BB:CC:DD:EE:FF"))
    }

    @Test fun unknownAddressStillNeedsPairing() {
        val policy = BleReconnectPolicy { "AA:BB:CC:DD:EE:FF" }
        assertFalse(policy.isKnownDevice("11:22:33:44:55:66"))
        assertTrue(policy.shouldShowPairingUi("11:22:33:44:55:66"))
        assertFalse(policy.shouldAcceptAsReconnect("11:22:33:44:55:66"))
    }
}
