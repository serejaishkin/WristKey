package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.*
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.security.SecurityManager
import com.wristkey.sensors.MotionDetector
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.*

class WristKeyBleService : Service() {

    companion object {
        private const val TAG = "WristKeyBLE"
        const val CHANNEL_ID = "wristkey_ble_channel"
        const val NOTIFICATION_ID = 1

        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val STATUS_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CLIENT_CONFIG_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

        const val STATUS_DISCONNECTED: Byte = 0x00
        const val STATUS_PAIRING: Byte = 0x01
        const val STATUS_AUTHENTICATED: Byte = 0x02

        // Pairing PIN — shown on watch screen, embedded in advertising
        var pairingPin: String = (1000..9999).random().toString()
            private set
    }

    private val binder = LocalBinder()
    private var bluetoothManager: BluetoothManager? = null
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiser: android.bluetooth.le.BluetoothLeAdvertiser? = null
    private var advertiseCallback: AdvertiseCallback? = null

    private val securityManager = SecurityManager()
    private val motionDetector by lazy { MotionDetector(this) }

    private var currentDevice: BluetoothDevice? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var statusCharacteristic: BluetoothGattCharacteristic? = null

    @Volatile
    private var lastUserPresentTime: Long = 0

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = bluetoothManager?.adapter
        motionDetector.start()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            startForeground(NOTIFICATION_ID, buildNotification("Initializing…"))
        } catch (e: Exception) {
            Log.e(TAG, "startForeground failed", e)
            stopSelf()
            return START_NOT_STICKY
        }

        startGattServer()
        startAdvertising()
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        stopAdvertising()
        stopGattServer()
        motionDetector.stop()
        super.onDestroy()
    }

    fun resetPairing() {
        Log.i(TAG, "Resetting pairing state")
        currentDevice?.let { device ->
            gattServer?.cancelConnection(device)
        }
        currentDevice = null
        updateStatus(STATUS_DISCONNECTED)
        securityManager.resetKeys()
        // Generate new PIN on reset
        pairingPin = (1000..9999).random().toString()
        stopAdvertising()
        startAdvertising()
        Log.i(TAG, "Pairing reset complete, new PIN: $pairingPin")
    }

    private fun startGattServer() {
        val gattServerCallback = object : BluetoothGattServerCallback() {
            override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
                when (newState) {
                    BluetoothProfile.STATE_CONNECTED -> {
                        Log.i(TAG, "Device connected: ${device.address}")
                        currentDevice = device
                        updateStatus(STATUS_AUTHENTICATED)
                        updateNotification("Connected to ${device.name ?: "PC"}")
                    }
                    BluetoothProfile.STATE_DISCONNECTED -> {
                        Log.i(TAG, "Device disconnected: ${device.address}")
                        if (currentDevice?.address == device.address) {
                            currentDevice = null
                            updateStatus(STATUS_DISCONNECTED)
                            updateNotification("Waiting for PC…")
                        }
                    }
                }
            }

            override fun onCharacteristicWriteRequest(
                device: BluetoothDevice,
                requestId: Int,
                characteristic: BluetoothGattCharacteristic,
                preparedWrite: Boolean,
                responseNeeded: Boolean,
                offset: Int,
                value: ByteArray?
            ) {
                if (characteristic.uuid == CHALLENGE_CHAR_UUID) {
                    handleChallenge(device, requestId, responseNeeded, value)
                } else {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                }
            }

            override fun onDescriptorWriteRequest(
                device: BluetoothDevice,
                requestId: Int,
                descriptor: BluetoothGattDescriptor,
                preparedWrite: Boolean,
                responseNeeded: Boolean,
                offset: Int,
                value: ByteArray?
            ) {
                if (descriptor.uuid == CLIENT_CONFIG_UUID) {
                    descriptor.value = value
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                }
            }
        }

        gattServer = bluetoothManager?.openGattServer(this, gattServerCallback)
        if (gattServer == null) {
            Log.e(TAG, "Failed to open GATT server")
            return
        }

        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)

        val challengeChar = BluetoothGattCharacteristic(
            CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        responseCharacteristic = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ
        ).apply {
            addDescriptor(BluetoothGattDescriptor(
                CLIENT_CONFIG_UUID,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
            ))
        }

        statusCharacteristic = BluetoothGattCharacteristic(
            STATUS_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ
        ).apply {
            addDescriptor(BluetoothGattDescriptor(
                CLIENT_CONFIG_UUID,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
            ))
            setValue(byteArrayOf(STATUS_DISCONNECTED))
        }

        service.addCharacteristic(challengeChar)
        service.addCharacteristic(responseCharacteristic)
        service.addCharacteristic(statusCharacteristic)

        gattServer?.addService(service)
        Log.i(TAG, "GATT server started with service $SERVICE_UUID")
    }

    private fun stopGattServer() {
        gattServer?.close()
        gattServer = null
    }

    private fun handleChallenge(
        device: BluetoothDevice,
        requestId: Int,
        responseNeeded: Boolean,
        value: ByteArray?
    ) {
        if (value == null || value.size < 24) {
            Log.w(TAG, "Invalid challenge length: ${value?.size}")
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_ATTRIBUTE_LENGTH, 0, null)
            }
            return
        }

        if (!motionDetector.isMoving) {
            Log.w(TAG, "Rejecting challenge: watch not in motion (possible relay attack)")
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            }
            return
        }

        val nonce = value.copyOfRange(0, 16)
        val timestamp = ByteBuffer.wrap(value.copyOfRange(16, 24)).order(ByteOrder.LITTLE_ENDIAN).long

        val now = System.currentTimeMillis() / 1000
        if (kotlin.math.abs(now - timestamp) > 30) {
            Log.w(TAG, "Challenge timestamp expired: $timestamp vs $now")
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            }
            return
        }

        val userPresent = isUserPresent()
        val userPresentByte: Byte = if (userPresent) 1 else 0

        val payload = nonce + value.copyOfRange(16, 24) + byteArrayOf(userPresentByte)

        val signature = try {
            securityManager.sign(payload)
        } catch (e: Exception) {
            Log.e(TAG, "Signing failed", e)
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            }
            return
        }

        val rawSignature = derToRaw(signature)
        val response = rawSignature + byteArrayOf(userPresentByte)

        responseCharacteristic?.value = response
        val notified = gattServer?.notifyCharacteristicChanged(device, responseCharacteristic, false) ?: false
        Log.i(TAG, "Challenge signed and notified. userPresent=$userPresent, notified=$notified")

        if (responseNeeded) {
            gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
        }
    }

    private fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        advertiser = adapter.bluetoothLeAdvertiser ?: return

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setConnectable(true)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .build()

        val pinBytes = pairingPin.toByteArray(Charsets.UTF_8)
        val data = AdvertiseData.Builder()
            .setIncludeTxPowerLevel(false)
            .setIncludeTxPowerLevel(false)
            .setIncludeDeviceName(true)
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .addManufacturerData(0xFFFF, pinBytes)
            .build()

        advertiseCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started successfully, PIN: $pairingPin")
                updateNotification("PIN: $pairingPin")
            }

            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        advertiser?.startAdvertising(settings, data, scanResponse, advertiseCallback!!)
    }

    private fun stopAdvertising() {
        advertiser?.stopAdvertising(advertiseCallback)
        advertiser = null
    }

    private fun updateStatus(status: Byte) {
        statusCharacteristic?.value = byteArrayOf(status)
        currentDevice?.let { device ->
            gattServer?.notifyCharacteristicChanged(device, statusCharacteristic, false)
        }
    }

    private fun updateNotification(text: String) {
        val notification = buildNotification(text)
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, notification)
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "WristKey BLE",
            NotificationManager.IMPORTANCE_LOW
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setOngoing(true)
            .build()
    }

    fun confirmUserPresent() {
        lastUserPresentTime = System.currentTimeMillis()
        Log.i(TAG, "User presence confirmed")
    }

    private fun isUserPresent(): Boolean {
        return System.currentTimeMillis() - lastUserPresentTime < 60_000
    }

    private fun derToRaw(der: ByteArray): ByteArray {
        if (der.size < 70 || der[0] != 0x30.toByte()) {
            Log.w(TAG, "Unexpected DER format, returning as-is")
            return der
        }
        var idx = 2
        if (der[idx] != 0x02.toByte()) return der
        idx++
        val rLen = der[idx].toInt() and 0xFF
        idx++
        val r = der.copyOfRange(idx, idx + rLen).let {
            if (it.size == 33 && it[0] == 0.toByte()) it.copyOfRange(1, 33) else it
        }
        idx += rLen
        if (der[idx] != 0x02.toByte()) return der
        idx++
        val sLen = der[idx].toInt() and 0xFF
        idx++
        val s = der.copyOfRange(idx, idx + sLen).let {
            if (it.size == 33 && it[0] == 0.toByte()) it.copyOfRange(1, 33) else it
        }

        val rPadded = ByteArray(32) { i -> if (i < 32 - r.size) 0 else r[i - (32 - r.size)] }
        val sPadded = ByteArray(32) { i -> if (i < 32 - s.size) 0 else s[i - (32 - s.size)] }
        return rPadded + sPadded
    }
}
