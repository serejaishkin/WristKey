package com.wristkey.ble

import org.junit.Assert.assertEquals
import org.junit.Test

class ProximityEngineTest {
    @Test
    fun `near signal needs confirmation`() {
        val engine = ProximityEngine(nearThreshold = -65, samplesToConfirm = 3)

        assertEquals(ProximityEngine.State.NEAR, engine.update(-60))
        assertEquals(ProximityEngine.State.NEAR, engine.update(-61))
        assertEquals(ProximityEngine.State.PRESENT, engine.update(-62))
    }

    @Test
    fun `away signal needs several samples`() {
        val engine = ProximityEngine(nearThreshold = -65, awayThreshold = -78, samplesToConfirm = 1, samplesToLeave = 3)
        assertEquals(ProximityEngine.State.PRESENT, engine.update(-60))
        assertEquals(ProximityEngine.State.SUSPECTED_AWAY, engine.update(-80))
        assertEquals(ProximityEngine.State.SUSPECTED_AWAY, engine.update(-81))
        assertEquals(ProximityEngine.State.AWAY, engine.update(-82))
    }

    @Test
    fun `hysteresis recovers before away confirmation`() {
        val engine = ProximityEngine(nearThreshold = -65, awayThreshold = -78, samplesToConfirm = 1, samplesToLeave = 3)
        engine.update(-60)
        engine.update(-80)
        assertEquals(ProximityEngine.State.PRESENT, engine.update(-64))
    }
}
