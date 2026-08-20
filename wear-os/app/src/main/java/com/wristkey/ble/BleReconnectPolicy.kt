package com.wristkey.ble

/**
 * Decides whether a newly connected BLE peer is already paired.
 * Pairing is identified by the persisted address/name pair for now;
 * cryptographic identity remains the authority for future unlock flows.
 */
class BleReconnectPolicy(
    private val pairedAddressProvider: () -> String?
) {
    fun isKnownDevice(address: String?): Boolean {
        if (address.isNullOrBlank()) return false
        return address.equals(pairedAddressProvider(), ignoreCase = true)
    }

    fun shouldShowPairingUi(address: String?): Boolean = !isKnownDevice(address)

    fun shouldAcceptAsReconnect(address: String?): Boolean = isKnownDevice(address)
}
