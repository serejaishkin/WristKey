package com.wristkey.ble

import kotlin.math.abs

/**
 * Lightweight RSSI -> proximity state machine.
 * RSSI is proximity evidence only; it is never authentication.
 */
class ProximityRssiTracker(
    private val nearThreshold: Int = -70,
    private val awayThreshold: Int = -82,
    private val samplesToConfirm: Int = 3,
    private val samplesToRecover: Int = 2,
    private val smoothingAlpha: Double = 0.35
) {
    enum class State { UNKNOWN, NEAR, PRESENT, SUSPECTED_AWAY, AWAY }

    data class Snapshot(
        val state: State,
        val rawRssi: Int?,
        val filteredRssi: Double?,
        val sampleCount: Int
    )

    private var filtered: Double? = null
    private var nearSamples = 0
    private var awaySamples = 0
    private var recoverSamples = 0
    private var state = State.UNKNOWN

    fun reset() {
        filtered = null
        nearSamples = 0
        awaySamples = 0
        recoverSamples = 0
        state = State.UNKNOWN
    }

    fun update(rssi: Int): Snapshot {
        filtered = filtered?.let { it + smoothingAlpha * (rssi - it) } ?: rssi.toDouble()
        val f = filtered ?: rssi.toDouble()

        when {
            f >= nearThreshold -> {
                nearSamples++
                awaySamples = 0
                if (state == State.SUSPECTED_AWAY || state == State.AWAY) {
                    recoverSamples++
                    if (recoverSamples >= samplesToRecover) {
                        state = State.PRESENT
                        recoverSamples = 0
                    }
                } else if (nearSamples >= samplesToConfirm) {
                    state = if (state == State.UNKNOWN) State.NEAR else State.PRESENT
                }
            }
            f <= awayThreshold -> {
                awaySamples++
                nearSamples = 0
                recoverSamples = 0
                when (state) {
                    State.PRESENT, State.NEAR -> if (awaySamples >= samplesToConfirm) state = State.SUSPECTED_AWAY
                    State.SUSPECTED_AWAY -> if (awaySamples >= samplesToConfirm * 2) state = State.AWAY
                    else -> Unit
                }
            }
            else -> {
                nearSamples = 0
                awaySamples = 0
                recoverSamples = 0
            }
        }

        return Snapshot(state, rssi, f, maxOf(nearSamples, awaySamples))
    }

    fun snapshot(): Snapshot = Snapshot(state, null, filtered, maxOf(nearSamples, awaySamples))

    fun isNearby(): Boolean = state == State.NEAR || state == State.PRESENT || state == State.SUSPECTED_AWAY

    companion object {
        fun isAbruptChange(previous: Int, current: Int, delta: Int = 15): Boolean = abs(current - previous) >= delta
    }
}
