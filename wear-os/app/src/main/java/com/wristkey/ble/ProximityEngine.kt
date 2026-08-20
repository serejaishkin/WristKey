package com.wristkey.ble

/**
 * Small, deterministic proximity state machine.
 *
 * RSSI is only a proximity signal. It must never be treated as device identity
 * or as proof that an unlock is authorized.
 *
 * This class deliberately does not lock/unlock Windows and does not perform
 * cryptographic authentication yet. It provides the state layer that can be
 * connected to the BLE service later.
 */
class ProximityEngine(
    private val nearThreshold: Int = -65,
    private val awayThreshold: Int = -78,
    private val samplesToConfirm: Int = 3,
    private val samplesToLeave: Int = 5
) {
    enum class State {
        UNKNOWN,
        NEAR,
        PRESENT,
        SUSPECTED_AWAY,
        AWAY
    }

    private var state = State.UNKNOWN
    private var nearSamples = 0
    private var awaySamples = 0
    private var lastRssi: Int? = null

    @Synchronized
    fun update(rssi: Int): State {
        lastRssi = rssi

        when (state) {
            State.UNKNOWN, State.AWAY -> {
                awaySamples = 0
                if (rssi >= nearThreshold) {
                    nearSamples++
                    state = if (nearSamples >= samplesToConfirm) State.PRESENT else State.NEAR
                } else {
                    nearSamples = 0
                }
            }

            State.NEAR, State.PRESENT -> {
                if (rssi <= awayThreshold) {
                    awaySamples++
                    if (state == State.PRESENT) state = State.SUSPECTED_AWAY
                    if (awaySamples >= samplesToLeave) state = State.AWAY
                } else {
                    awaySamples = 0
                    nearSamples = minOf(samplesToConfirm, nearSamples + 1)
                    if (state == State.NEAR && nearSamples >= samplesToConfirm) {
                        state = State.PRESENT
                    }
                }
            }

            State.SUSPECTED_AWAY -> {
                if (rssi >= nearThreshold) {
                    awaySamples = 0
                    nearSamples = samplesToConfirm
                    state = State.PRESENT
                } else if (rssi <= awayThreshold) {
                    awaySamples++
                    if (awaySamples >= samplesToLeave) state = State.AWAY
                } else {
                    // Hysteresis band: keep the suspected-away state.
                    awaySamples++
                    if (awaySamples >= samplesToLeave) state = State.AWAY
                }
            }
        }

        return state
    }

    @Synchronized
    fun reset(): State {
        state = State.UNKNOWN
        nearSamples = 0
        awaySamples = 0
        lastRssi = null
        return state
    }

    @Synchronized
    fun state(): State = state

    @Synchronized
    fun lastRssi(): Int? = lastRssi
}
