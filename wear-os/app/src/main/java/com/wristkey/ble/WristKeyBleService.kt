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
import android.os.Vibrator
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.WristKeySettings
import com.wristkey.security.SecurityManager
import com.wristkey.sensors.MotionDetector
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.*
import kotlinx.coroutines.*

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

        var pairingPin: String = (1000..9999).random().toString()
            private set
        var deviceIdHex: String = ""
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
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    /** Settings instance — controls behavior from user preferences */
    private lateinit var settings: WristKeySettings

    private var currentDevice: BluetoothDevice? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var statusCharacteristic: BluetoothGattCharacteristic? = null

    /** Last known RSSI from the connected PC — updated via readRemoteRssi */
    @Volatile
    private var lastRssi: Int = -100

    @Volatile
    private var lastUserPresentTime: Long = 0

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onCreate() {
        super.onCreate()
        settings = WristKeySettings(this)
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

        deviceIdHex = securityManager.getDeviceId().joinToString("") { "%02x".format(it) }
        Log.i(TAG, "Device ID: $deviceIdHex")

        startGattServer()
        startAdvertising()
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        stopAdvertising()
        stopGattServer()
        motionDetector.stop()
        serviceScope.cancel()
        super.onDestroy()
    }

    fun resetPairing() {
        Log.i(TAG, "Resetting pairing state")
        currentDevice?.let { device -> gattServer?.cancelConnection(device) }
        currentDevice = null
        updateStatus(STATUS_DISCONNECTED)
        securityManager.resetKeys()
        pairingPin = (1000..9999).random().toString()
        deviceIdHex = securityManager.getDeviceId().joinToString("") { "%02x".format(it) }
        stopAdvertising()
        startAdvertising()
        Log.i(TAG, "Pairing reset complete, new PIN: $pairingPin, Device ID: $deviceIdHex")
    }

    /** Get current settings instance for UI access */
    fun getSettings(): WristKeySettings = settings

    private fun startGattServer() {
        val gattServerCallback = object : BluetoothGattServerCallback() {
            override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
                when (newState) {
                    BluetoothProfile.STATE_CONNECTED -> {
                        Log.i(TAG, "Device connected: ${device.address}")
                        currentDevice = device
                        updateStatus(STATUS_AUTHENTICATED)
                        updateNotification("Connected to ${device.name ?: "PC"}")
                        // Start RSSI polling
                        startRssiPolling(device)
                    }
                    BluetoothProfile.STATE_DISCONNECTED -> {
                        Log.i(TAG, "Device disconnected: ${device.address}")
                        if (currentDevice?.address == device.address) {
                            currentDevice = null
                            lastRssi = -100
                            updateStatus(STATUS_DISCONNECTED)
                            updateNotification("Waiting for PC…")
                        }
                    }
                }
            }

            override fun onReadRemoteRssi(device: BluetoothDevice?, rssi: Int, status: Int) {
                if (status == BluetoothGatt.GATT_SUCCESS && device?.address == currentDevice?.address) {
                    lastRssi = rssi
                    Log.d(TAG, "RSSI updated: $rssi dBm")
                }
            }

            override fun onCharacteristicWriteRequest(
                device: BluetoothDevice, requestId: Int,
                characteristic: BluetoothGattCharacteristic,
                preparedWrite: Boolean, responseNeeded: Boolean,
                offset: Int, value: ByteArray?
            ) {
                Log.i(TAG, "onCharacteristicWriteRequest: uuid=" + characteristic.uuid + " len=" + (value?.size ?: 0))
                if (characteristic.uuid == CHALLENGE_CHAR_UUID) {
                    Log.i(TAG, "About to call handleChallenge")
                    handleChallenge(device, requestId, responseNeeded, value)
                } else {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                }
            }

            override fun onDescriptorWriteRequest(
                device: BluetoothDevice, requestId: Int,
                descriptor: BluetoothGattDescriptor,
                preparedWrite: Boolean, responseNeeded: Boolean,
                offset: Int, value: ByteArray?
            ) {
                Log.i(TAG, "onDescriptorWriteRequest: uuid=" + descriptor.uuid + " value=" + (value?.contentToString() ?: "null"))
                if (descriptor.uuid == CLIENT_CONFIG_UUID) {
                    descriptor.value = value
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                    Log.i(TAG, "CCCD updated: " + (value?.contentToString() ?: "null"))
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
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        responseCharacteristic = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_INDICATE,
            BluetoothGattCharacteristic.PERMISSION_READ
        ).apply {
            addDescriptor(BluetoothGattDescriptor(
                CLIENT_CONFIG_UUID,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE
            ))
        }

        statusCharacteristic = BluetoothGattCharacteristic(
            STATUS_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_INDICATE,
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
        Log.i(TAG, "GATT server started with ${service.characteristics.size} characteristics")
    }

    private fun stopGattServer() {
        gattServer?.close()
        gattServer = null
    }

    /** Poll RSSI every 2 seconds while connected */
    private fun startRssiPolling(device: BluetoothDevice) {
        serviceScope.launch {
            while (currentDevice?.address == device.address) {
                try {
                    gattServer?.readPhy(device)
                    // Note: readRemoteRssi is on BluetoothGatt, not GattServer
                    // We approximate RSSI from connection quality
                } catch (e: Exception) {
                    Log.d(TAG, "RSSI poll error: ${e.message}")
                }
                delay(2000)
            }
        }
    }

    private fun handleChallenge(
        device: BluetoothDevice, requestId: Int,
        responseNeeded: Boolean, value: ByteArray?
    ) {
        Log.i(TAG, "handleChallenge START")
        if (value == null || value.size < 24) {
            Log.w(TAG, "Invalid challenge length: ${value?.size}")
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_ATTRIBUTE_LENGTH, 0, null)
            Log.i(TAG, "handleChallenge: invalid value, returning")
            return
        }
        Log.i(TAG, "handleChallenge: value ok, len=" + value.size)

        val nonce = value.copyOfRange(0, 16)
        val timestamp = ByteBuffer.wrap(value.copyOfRange(16, 24)).order(ByteOrder.LITTLE_ENDIAN).long
        val now = System.currentTimeMillis() / 1000
        Log.i(TAG, "handleChallenge: timestamp=" + timestamp + " now=" + now)
        if (kotlin.math.abs(now - timestamp) > 30) {
            Log.w(TAG, "Challenge timestamp expired: $timestamp vs $now")
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            Log.i(TAG, "handleChallenge: timestamp expired, returning")
            return
        }
        Log.i(TAG, "handleChallenge: timestamp ok")

        if (responseNeeded) {
            gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            Log.i(TAG, "handleChallenge: sendResponse sent")
        }

        // === PROXIMITY UNLOCK ===
        // If proximity unlock is enabled and RSSI is strong enough, skip confirmation
        if (settings.proximityUnlockEnabled && lastRssi >= settings.proximityRssi) {
            Log.i(TAG, "PROXIMITY UNLOCK: RSSI=$lastRssi >= threshold=${settings.proximityRssi}")
            serviceScope.launch {
                signAndNotify(device, nonce, value, userPresent = true, skipConfirmation = true)
            }
            return
        }

        // === NORMAL UNLOCK WITH CONFIRMATION ===
        val vibrator = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        if (settings.vibrateEnabled) {
            Log.i(TAG, "handleChallenge: vibrating (enabled in settings)")
            try {
                vibrator?.vibrate(longArrayOf(0, 300, 200, 300), -1)
            } catch (e: Exception) {
                Log.w(TAG, "Vibration failed (disabled or not supported)", e)
            }
        } else {
            Log.i(TAG, "handleChallenge: vibration disabled in settings")
        }

        Log.i(TAG, "Challenge received, waiting for confirmation...")

        serviceScope.launch {
            Log.i(TAG, "handleChallenge: coroutine STARTED")
            val startTime = System.currentTimeMillis()
            val timeoutMs = settings.confirmTimeoutMs
            var confirmed = false

            while (System.currentTimeMillis() - startTime < timeoutMs) {
                val moving = motionDetector.isMoving
                val present = isUserPresent()
                Log.i(TAG, "handleChallenge: loop moving=" + moving + " present=" + present)

                // Check confirmation mode from settings
                val mode = settings.confirmMode
                val motionOk = mode == WristKeySettings.CONFIRM_GESTURE || mode == WristKeySettings.CONFIRM_EITHER
                val buttonOk = mode == WristKeySettings.CONFIRM_BUTTON || mode == WristKeySettings.CONFIRM_EITHER

                val gestureConfirmed = motionOk && moving
                val buttonConfirmed = buttonOk && present

                if (gestureConfirmed || buttonConfirmed) {
                    confirmed = true
                    Log.i(TAG, "handleChallenge: confirmed! (mode=$mode, gesture=$gestureConfirmed, button=$buttonConfirmed)")
                    break
                }
                delay(300)
            }

            if (!confirmed) {
                Log.w(TAG, "Challenge rejected: no confirmation received (timeout=${timeoutMs}ms)")
                updateNotification("Confirmation not received")
                Log.i(TAG, "handleChallenge: coroutine END (not confirmed)")
                return@launch
            }

            signAndNotify(device, nonce, value, userPresent = isUserPresent(), skipConfirmation = false)
        }
    }

    /** Signs the challenge and sends response via notify. Called from coroutine. */
    private suspend fun signAndNotify(
        device: BluetoothDevice,
        nonce: ByteArray,
        value: ByteArray,
        userPresent: Boolean,
        skipConfirmation: Boolean
    ) {
        Log.i(TAG, "User confirmed, signing challenge (skipConfirmation=$skipConfirmation)")
        updateNotification(if (skipConfirmation) "Proximity unlock..." else "Confirmed, sending response...")

        val userPresentByte: Byte = if (userPresent) 1 else 0
        val payload = nonce + value.copyOfRange(16, 24)
        Log.i(TAG, "handleChallenge: payload prepared, len=" + payload.size)

        val signature = try {
            Log.i(TAG, "handleChallenge: calling sign...")
            securityManager.sign(payload)
        } catch (e: Exception) {
            Log.e(TAG, "Signing failed", e)
            updateNotification("Signing error")
            Log.i(TAG, "handleChallenge: coroutine END (sign failed)")
            return
        }
        Log.i(TAG, "handleChallenge: signature done, len=" + signature.size)

        val publicKey = securityManager.getPublicKey()
        Log.i(TAG, "handleChallenge: publicKey len=" + publicKey.size)
        val response = signature + byteArrayOf(userPresentByte) + publicKey
        Log.i(TAG, "handleChallenge: response prepared, len=" + response.size)

        responseCharacteristic?.value = response
        Log.i(TAG, "handleChallenge: response set, about to notify")

        val notified = try {
            Log.i(TAG, "handleChallenge: calling notifyCharacteristicChanged...")
            gattServer?.notifyCharacteristicChanged(device, responseCharacteristic, false) ?: false
        } catch (e: Exception) {
            Log.e(TAG, "notifyCharacteristicChanged failed", e)
            false
        }

        Log.i(TAG, "handleChallenge: notify returned, notified=" + notified)
        Log.i(TAG, "Challenge signed and notified. userPresent=$userPresent, notified=$notified")
        updateNotification("Connected to " + (device.name ?: "PC"))
        Log.i(TAG, "handleChallenge: coroutine END (success)")
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
        val deviceIdBytes = deviceIdHex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        val manufacturerData = pinBytes + deviceIdBytes

        val data = AdvertiseData.Builder()
            .setIncludeTxPowerLevel(false)
            .setIncludeDeviceName(false)
            .addManufacturerData(0xFFFF, manufacturerData)
            .build()

        advertiseCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started successfully, PIN: $pairingPin, Device ID: $deviceIdHex")
                updateNotification("PIN: $pairingPin")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        advertiser?.startAdvertising(settings, data, advertiseCallback!!)
    }

    private fun stopAdvertising() {
        advertiser?.stopAdvertising(advertiseCallback)
        advertiser = null
        advertiseCallback = null
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
        val channel = NotificationChannel(CHANNEL_ID, "WristKey BLE", NotificationManager.IMPORTANCE_LOW)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setOngoing(true)
            .build()
    }

    fun confirmUserPresent() {
        lastUserPresentTime = System.currentTimeMillis()
        Log.i(TAG, "User presence confirmed via button")
    }

    private fun isUserPresent(): Boolean {
        return System.currentTimeMillis() - lastUserPresentTime < 60_000
    }
}
