package com.wristkey.ble

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ProximityRssiTrackerTest {
    @Test fun nearNeedsMultipleSamples() {
        val tracker = ProximityRssiTracker()
        assertEquals(ProximityRssiTracker.State.UNKNOWN, tracker.update(-65).state)
        assertEquals(ProximityRssiTracker.State.UNKNOWN, tracker.update(-65).state)
        assertEquals(ProximityRssiTracker.State.NEAR, tracker.update(-65).state)
    }

    @Test fun awayRequiresConfirmation() {
        val tracker = ProximityRssiTracker()
        repeat(3) { tracker.update(-60) }
        assertEquals(ProximityRssiTracker.State.NEAR, tracker.snapshot().state)
        repeat(3) { tracker.update(-90) }
        assertEquals(ProximityRssiTracker.State.SUSPECTED_AWAY, tracker.snapshot().state)
        repeat(3) { tracker.update(-90) }
        assertEquals(ProximityRssiTracker.State.AWAY, tracker.snapshot().state)
    }

    @Test fun recoveryNeedsTwoGoodSamples() {
        val tracker = ProximityRssiTracker()
        repeat(3) { tracker.update(-60) }
        repeat(3) { tracker.update(-90) }
        assertTrue(tracker.snapshot().state == ProximityRssiTracker.State.SUSPECTED_AWAY || tracker.snapshot().state == ProximityRssiTracker.State.AWAY)
        tracker.update(-55)
        assertEquals(ProximityRssiTracker.State.SUSPECTED_AWAY, tracker.snapshot().state)
        tracker.update(-55)
        assertEquals(ProximityRssiTracker.State.PRESENT, tracker.snapshot().state)
    }

    @Test fun abruptRssiChangeCanBeDetected() {
        assertTrue(ProximityRssiTracker.isAbruptChange(-55, -75))
    }
}
