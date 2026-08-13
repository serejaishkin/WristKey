package com.wristkey.ble

import android.app.Service
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.util.Log
import com.wristkey.WristKeySettings
import kotlinx.coroutines.*
import java.nio.ByteBuffer
import java.util.UUID

class WristKeyBleService : Service() {
    companion object {
        const val TAG = "WristKeyBleService"
        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val STATUS_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CONFIG_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")

        const val CMD_START_CALIBRATION: Byte = 0x01
        const val CMD_CALIBRATION_RESULT: Byte = 0x02
        const val CMD_CANCEL_CALIBRATION: Byte = 0x03
    }

    private val binder = LocalBinder()
    private var bluetoothGatt: BluetoothGatt? = null
    private var settings: WristKeySettings? = null
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var calibrationInProgress = false
    private var lastRssiFromPc = -100
    private var lastRssi = -100
    private var pairedDeviceName: String? = null
    private var isPairedState = false

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onCreate() {
        super.onCreate()
        settings = WristKeySettings(this)
        isPairedState = settings?.pairedDevices?.isNotEmpty() == true
    }

    override fun onDestroy() {
        super.onDestroy()
        serviceScope.cancel()
        bluetoothGatt?.close()
    }

    fun isPaired(): Boolean = isPairedState

    fun getDeviceName(): String = pairedDeviceName ?: "Not connected"

    fun getLastRssi(): Int = lastRssi

    fun sendUnlockChallenge() {
        Log.i(TAG, "sendUnlockChallenge called")
        // TODO: implement BLE write to PC for unlock
    }

    fun forgetDevice() {
        Log.i(TAG, "forgetDevice called")
        settings?.clearPairedDevices()
        isPairedState = false
        pairedDeviceName = null
    }

    fun requestCalibration() {
        Log.i(TAG, "requestCalibration called")
        calibrationInProgress = true
        lastRssiFromPc = -100
    }

    fun cancelCalibration() {
        Log.i(TAG, "cancelCalibration called")
        calibrationInProgress = false
    }

    fun isCalibrationInProgress(): Boolean = calibrationInProgress

    fun getLastRssiFromPc(): Int = lastRssiFromPc

    private fun connectToDevice(device: BluetoothDevice) {
        bluetoothGatt = device.connectGatt(this, false, gattCallback)
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            if (newState == BluetoothProfile.STATE_CONNECTED) {
                Log.i(TAG, "Connected to GATT server.")
                gatt.discoverServices()
            } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                Log.i(TAG, "Disconnected from GATT server.")
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                val service = gatt.getService(SERVICE_UUID)
                service?.getCharacteristic(CONFIG_CHAR_UUID)?.let { char ->
                    gatt.setCharacteristicNotification(char, true)
                    val descriptor = char.getDescriptor(UUID.fromString("00002902-0000-1000-8000-00805f9b34fb"))
                    descriptor?.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                    gatt.writeDescriptor(descriptor)
                }
            }
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray
        ) {
            if (characteristic.uuid == CONFIG_CHAR_UUID) {
                handleConfigCommand(value)
            }
        }

        override fun onReadRemoteRssi(gatt: BluetoothGatt?, rssi: Int, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                lastRssi = rssi
            }
        }
    }

    private fun handleConfigCommand(value: ByteArray) {
        if (value.isEmpty()) return

        val cmd = value[0]
        when (cmd) {
            CMD_START_CALIBRATION -> {
                Log.i(TAG, "PC requested calibration start")
                calibrationInProgress = true
                lastRssiFromPc = -100
            }
            CMD_CALIBRATION_RESULT -> {
                if (value.size >= 2) {
                    val rssi = value[1].toInt()
                    Log.i(TAG, "PC sent calibration result: $rssi dBm")
                    settings?.saveCalibration(rssi)
                    calibrationInProgress = false
                }
            }
            CMD_CANCEL_CALIBRATION -> {
                Log.i(TAG, "PC cancelled calibration")
                calibrationInProgress = false
            }
            else -> {
                if (value.size == 1) {
                    lastRssiFromPc = value[0].toInt()
                    Log.d(TAG, "RSSI update from PC: $lastRssiFromPc dBm")
                }
            }
        }
    }
}
