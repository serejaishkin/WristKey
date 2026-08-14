package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.ParcelUuid
import android.os.VibrationEffect
import android.os.Vibrator
import android.util.Log
import android.widget.Toast
import com.wristkey.WristKeySettings
import com.wristkey.security.SecurityManager
import com.wristkey.sensors.MotionDetector
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

@Suppress("DEPRECATION")
class WristKeyBleService : Service() {

    private val binder = LocalBinder()
    private var bluetoothGattServer: BluetoothGattServer? = null
    private var bluetoothLeAdvertiser: BluetoothLeAdvertiser? = null
    private var challengeCharacteristic: BluetoothGattCharacteristic? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var configCharacteristic: BluetoothGattCharacteristic? = null

    private var connectedDevice: BluetoothDevice? = null
    private var pairedDeviceAddress: String? = null
    private var lastRssi: Int = 0

    private val pendingUserPresent = AtomicBoolean(false)
    private val userPresenceTimeoutHandler = Handler(Looper.getMainLooper())
    private var userPresenceRunnable: Runnable? = null

    private lateinit var securityManager: SecurityManager
    private lateinit var motionDetector: MotionDetector
    private lateinit var settings: WristKeySettings
    private val mainHandler = Handler(Looper.getMainLooper())

    private val SERVICE_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
    private val CHALLENGE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
    private val RESPONSE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
    private val CONFIG_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
    private val CCCD_UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "onCreate called")
        securityManager = SecurityManager()
        motionDetector = MotionDetector(this)
        settings = WristKeySettings(this)
        startForegroundService()
        startGattServer()
        motionDetector.start()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "onStartCommand called")
        startForegroundService()
        if (bluetoothGattServer == null) startGattServer()
        if (bluetoothLeAdvertiser == null) startAdvertising()
        return START_STICKY
    }

    private fun startForegroundService() {
        val channelId = "wristkey_ble"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                channelId, "WristKey BLE",
                NotificationManager.IMPORTANCE_LOW
            )
            (getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
                .createNotificationChannel(channel)
        }
        val notification = Notification.Builder(this, channelId)
            .setContentTitle("WristKey")
            .setContentText("GATT server running")
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .build()
        startForeground(1, notification)
    }

    private fun startGattServer() {
        try {
            val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
            val adapter = bluetoothManager.adapter ?: run {
                Log.e(TAG, "Bluetooth adapter not available")
                return
            }
            if (!adapter.isEnabled) {
                Log.e(TAG, "Bluetooth adapter is disabled")
                return
            }

            val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)

            challengeCharacteristic = BluetoothGattCharacteristic(
                CHALLENGE_CHAR_UUID,
                BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
                BluetoothGattCharacteristic.PERMISSION_WRITE
            )

            responseCharacteristic = BluetoothGattCharacteristic(
                RESPONSE_CHAR_UUID,
                BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                BluetoothGattCharacteristic.PERMISSION_READ
            ).apply {
                addDescriptor(BluetoothGattDescriptor(CCCD_UUID, BluetoothGattDescriptor.PERMISSION_WRITE))
            }

            configCharacteristic = BluetoothGattCharacteristic(
                CONFIG_CHAR_UUID,
                BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_READ or BluetoothGattCharacteristic.PERMISSION_WRITE
            )

            service.addCharacteristic(challengeCharacteristic)
            service.addCharacteristic(responseCharacteristic)
            service.addCharacteristic(configCharacteristic)

            bluetoothGattServer = bluetoothManager.openGattServer(this, gattServerCallback)
            if (bluetoothGattServer == null) {
                Log.e(TAG, "openGattServer returned null - GATT server not created")
                return
            }
            val added = bluetoothGattServer?.addService(service)
            Log.i(TAG, "addService returned: $added (service=$SERVICE_UUID)")
        } catch (e: SecurityException) {
            Log.e(TAG, "SecurityException in startGattServer - missing BLUETOOTH_CONNECT permission?", e)
        } catch (e: Exception) {
            Log.e(TAG, "Unexpected error in startGattServer", e)
        }
    }

    private fun startAdvertising() {
        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        val adapter = bluetoothManager.adapter ?: run {
            Log.e(TAG, "Bluetooth adapter not available for advertising")
            return
        }
        bluetoothLeAdvertiser = adapter.bluetoothLeAdvertiser
        if (bluetoothLeAdvertiser == null) {
            Log.e(TAG, "BluetoothLeAdvertiser is null - advertising not supported")
            return
        }

        val advSettings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setConnectable(true)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .build()

        val deviceId = securityManager.getDeviceId()
        val manufData = ByteArray(8)
        System.arraycopy("WRST".toByteArray(), 0, manufData, 0, 4)
        System.arraycopy(deviceId, 0, manufData, 4, 4.coerceAtMost(deviceId.size))

        // FIX: split advertisement and scan response to avoid ADVERTISE_FAILED_DATA_TOO_LARGE
        // Legacy advertisement limit = 31 bytes. 128-bit UUID (16b) + manuf data (8b) + headers = overflow.
        val advData = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addManufacturerData(0xFFFF, "WRST".toByteArray())
            .build()

        val scanResponse = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .addManufacturerData(0xFFFF, manufData)
            .build()

        Log.i(TAG, "Starting advertising with scan response (split payload)")
        bluetoothLeAdvertiser?.startAdvertising(advSettings, advData, scanResponse, advertiseCallback)
        Log.i(TAG, "startAdvertising called (with scan response)")
    }

    private fun stopAdvertising() {
        bluetoothLeAdvertiser?.stopAdvertising(advertiseCallback)
        bluetoothLeAdvertiser = null
        Log.i(TAG, "Advertising stopped")
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
            Log.i(TAG, "Advertising started successfully")
        }

        override fun onStartFailure(errorCode: Int) {
            Log.e(TAG, "Advertising failed to start: errorCode=$errorCode")
            when (errorCode) {
                ADVERTISE_FAILED_DATA_TOO_LARGE -> Log.e(TAG, "ADVERTISE_FAILED_DATA_TOO_LARGE")
                ADVERTISE_FAILED_TOO_MANY_ADVERTISERS -> Log.e(TAG, "ADVERTISE_FAILED_TOO_MANY_ADVERTISERS")
                ADVERTISE_FAILED_ALREADY_STARTED -> Log.e(TAG, "ADVERTISE_FAILED_ALREADY_STARTED")
                ADVERTISE_FAILED_INTERNAL_ERROR -> Log.e(TAG, "ADVERTISE_FAILED_INTERNAL_ERROR")
                ADVERTISE_FAILED_FEATURE_UNSUPPORTED -> Log.e(TAG, "ADVERTISE_FAILED_FEATURE_UNSUPPORTED")
            }
        }
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onServiceAdded(status: Int, service: BluetoothGattService?) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                Log.i(TAG, "onServiceAdded SUCCESS: ${service?.uuid}")
                startAdvertising()
            } else {
                Log.e(TAG, "onServiceAdded FAILED: status=$status, uuid=${service?.uuid}")
            }
        }

        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    Log.i(TAG, "Device connected: addr=${device?.address} name=${device?.name}")
                    connectedDevice = device
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    Log.i(TAG, "Device disconnected: addr=${device?.address}")
                    if (connectedDevice?.address == device?.address) {
                        connectedDevice = null
                    }
                }
            }
        }

        override fun onNotificationSent(device: BluetoothDevice?, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                Log.i(TAG, "onNotificationSent SUCCESS to addr=${device?.address}")
            } else {
                Log.e(TAG, "onNotificationSent FAILED: status=$status to addr=${device?.address}")
            }
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice?, requestId: Int, offset: Int,
            characteristic: BluetoothGattCharacteristic?
        ) {
            Log.i(TAG, "onCharacteristicReadRequest: ${characteristic?.uuid} from addr=${device?.address}")
            if (characteristic?.uuid == CONFIG_CHAR_UUID) {
                val value = byteArrayOf(0x01, 0x00)
                bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, value)
            } else {
                bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_READ_NOT_PERMITTED, 0, null)
            }
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice?, requestId: Int,
            characteristic: BluetoothGattCharacteristic?,
            preparedWrite: Boolean, responseNeeded: Boolean,
            offset: Int, value: ByteArray?
        ) {
            Log.i(TAG, "onCharacteristicWriteRequest: ${characteristic?.uuid}, ${value?.size} bytes from addr=${device?.address}")
            if (characteristic?.uuid == CHALLENGE_CHAR_UUID) {
                value?.let { processChallenge(it) }
                if (responseNeeded) {
                    bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                }
            } else if (characteristic?.uuid == CONFIG_CHAR_UUID) {
                value?.let { processConfig(it) }
                if (responseNeeded) {
                    bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                }
            } else {
                if (responseNeeded) {
                    bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_WRITE_NOT_PERMITTED, 0, null)
                }
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice?, requestId: Int,
            descriptor: BluetoothGattDescriptor?,
            preparedWrite: Boolean, responseNeeded: Boolean,
            offset: Int, value: ByteArray?
        ) {
            Log.i(TAG, "onDescriptorWriteRequest: ${descriptor?.uuid} from addr=${device?.address}")
            descriptor?.let {
                if (it.uuid == CCCD_UUID) {
                    it.value = value
                    if (responseNeeded) {
                        bluetoothGattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
                    }
                }
            }
        }
    }

    private fun processChallenge(challenge: ByteArray) {
        Log.i(TAG, "processChallenge: ${challenge.size} bytes")

        if (challenge.size < 16) {
            Log.e(TAG, "Challenge too short: ${challenge.size} bytes (expected >= 16)")
            return
        }

        try {
            val userPresent = when (settings.confirmMode) {
                WristKeySettings.CONFIRM_GESTURE -> motionDetector.isMoving
                WristKeySettings.CONFIRM_BUTTON -> {
                    val pending = pendingUserPresent.getAndSet(false)
                    if (pending) {
                        Log.i(TAG, "User presence confirmed via button request")
                    }
                    pending
                }
                else -> motionDetector.isMoving
            }

            if (!userPresent) {
                Log.w(TAG, "Challenge rejected: user presence not confirmed (mode=${settings.confirmMode})")
            }

            val signature = securityManager.sign(challenge)
            Log.i(TAG, "Signature generated: ${signature.size} bytes")

            val publicKey = securityManager.getPublicKey()
            Log.i(TAG, "Public key: ${publicKey.size} bytes")

            val response = ByteArray(130)
            System.arraycopy(signature, 0, response, 0, 64)
            response[64] = if (userPresent) 1 else 0
            System.arraycopy(publicKey, 0, response, 65, 65)

            Log.i(TAG, "Response built: ${response.size} bytes (sig=${signature.size}, user_present=$userPresent, pubkey=${publicKey.size})")
            sendResponse(response)

            if (settings.vibrateEnabled && userPresent) {
                vibrate()
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to process challenge", e)
        }
    }

    private fun processConfig(data: ByteArray) {
        when (data.firstOrNull()?.toInt()) {
            0x01 -> {
                Log.i(TAG, "Calibration START requested by PC")
                mainHandler.post {
                    Toast.makeText(this, "Приложите часы к ПК\nДержите 10 секунд", Toast.LENGTH_LONG).show()
                }
            }
            0x02 -> {
                if (data.size >= 2) {
                    val threshold = data[1].toByte().toInt()
                    Log.i(TAG, "Calibration RESULT received: threshold=$threshold dBm")
                    settings.saveCalibration(threshold)
                    mainHandler.post {
                        Toast.makeText(this, "✅ Калибровка: $threshold dBm", Toast.LENGTH_LONG).show()
                    }
                }
            }
            0x03 -> {
                Log.i(TAG, "Calibration CANCELLED by PC")
                mainHandler.post {
                    Toast.makeText(this, "Калибровка отменена", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    private fun sendResponse(data: ByteArray) {
        val device = connectedDevice ?: return
        responseCharacteristic?.value = data
        val notified = bluetoothGattServer?.notifyCharacteristicChanged(device, responseCharacteristic, false)
        Log.i(TAG, "notifyCharacteristicChanged to addr=${device.address}: $notified")
    }

    private fun vibrate() {
        val vibrator = getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        vibrator?.let {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                it.vibrate(VibrationEffect.createOneShot(100, VibrationEffect.DEFAULT_AMPLITUDE))
            } else {
                it.vibrate(100)
            }
        }
    }

    fun requestUserPresence() {
        Log.i(TAG, "requestUserPresence: user explicitly requested unlock")
        pendingUserPresent.set(true)

        userPresenceRunnable?.let { userPresenceTimeoutHandler.removeCallbacks(it) }

        val runnable = Runnable {
            if (pendingUserPresent.getAndSet(false)) {
                Log.i(TAG, "User presence request expired (10s timeout)")
            }
        }
        userPresenceRunnable = runnable
        userPresenceTimeoutHandler.postDelayed(runnable, 10000)

        mainHandler.post {
            Toast.makeText(this, "✅ Разблокировка разрешена (10 сек)", Toast.LENGTH_SHORT).show()
        }
        if (settings.vibrateEnabled) {
            vibrate()
        }
    }

    fun sendUnlockChallenge() {
        Log.i(TAG, "sendUnlockChallenge deprecated, use requestUserPresence")
        requestUserPresence()
    }

    fun requestCalibration() {
        Log.i(TAG, "requestCalibration: calibration is managed by PC via CONFIG characteristic")
    }

    fun isPaired(): Boolean = pairedDeviceAddress != null
    fun getDeviceName(): String = connectedDevice?.name ?: "Not connected"
    fun getLastRssi(): String = if (lastRssi != 0) "$lastRssi" else "--"
    fun isAdvertising(): Boolean = bluetoothLeAdvertiser != null
    fun getAdvertisePin(): String = "----"
    fun getConnectedDeviceAddress(): String = connectedDevice?.address ?: "--"

    fun forgetDevice() {
        pairedDeviceAddress = null
        settings.clearPairedDevices()
        Log.i(TAG, "Device forgotten")
    }

    override fun onDestroy() {
        super.onDestroy()
        Log.i(TAG, "onDestroy called")
        userPresenceRunnable?.let { userPresenceTimeoutHandler.removeCallbacks(it) }
        motionDetector.stop()
        stopAdvertising()
        bluetoothGattServer?.close()
        bluetoothGattServer = null
    }

    companion object {
        private const val TAG = "WristKeyBleService"
    }
}
