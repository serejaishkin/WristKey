package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import com.wristkey.WristKeySettings
import kotlinx.coroutines.*
import java.util.UUID

class WristKeyBleService : Service() {
    companion object {
        const val TAG = "WristKeyBleService"
        const val NOTIF_CHANNEL_ID = "wristkey_ble"
        const val NOTIF_ID = 1
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

    // --- BLE Advertising ---
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var advertiser: BluetoothLeAdvertiser? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var currentPin: String = "0000"
    private var advertiseDeviceId: ByteArray = byteArrayOf(0, 0, 0, 0)
    private var isAdvertising = false

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "onCreate")
        settings = WristKeySettings(this)
        isPairedState = settings?.pairedDevices?.isNotEmpty() == true

        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
        bluetoothAdapter = bluetoothManager?.adapter
        advertiser = bluetoothAdapter?.bluetoothLeAdvertiser

        currentPin = (1000..9999).random().toString()
        advertiseDeviceId = ByteArray(4) { (0..255).random().toByte() }

        Log.i(TAG, "Service created. PIN=$currentPin")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        Log.i(TAG, "onStartCommand")
        createNotificationChannel()
        val notification = Notification.Builder(this, NOTIF_CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText("BLE service running")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .build()
        startForeground(NOTIF_ID, notification)
        return START_STICKY
    }

    override fun onDestroy() {
        Log.i(TAG, "onDestroy")
        super.onDestroy()
        stopAdvertising()
        serviceScope.cancel()
        bluetoothGatt?.close()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                NOTIF_CHANNEL_ID,
                "WristKey BLE",
                NotificationManager.IMPORTANCE_LOW
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(channel)
        }
    }

    // --- Advertising API ---

    fun getAdvertisePin(): String = currentPin

    fun isAdvertising(): Boolean = isAdvertising

    fun startAdvertising() {
        try {
            val adv = advertiser ?: run {
                Log.e(TAG, "BluetoothLeAdvertiser not available")
                return
            }

            stopAdvertising()

            val settings = AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
                .setConnectable(true)
                .build()

            val pinBytes = currentPin.toByteArray(Charsets.UTF_8)
            val manufacturerData = pinBytes + advertiseDeviceId

            val data = AdvertiseData.Builder()
                .setIncludeDeviceName(true)
                .addManufacturerData(0xFFFF, manufacturerData)
                .addServiceUuid(ParcelUuid(SERVICE_UUID))
                .build()

            advertiseCallback = object : AdvertiseCallback() {
                override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                    isAdvertising = true
                    Log.i(TAG, "Advertising started. PIN=$currentPin")
                }

                override fun onStartFailure(errorCode: Int) {
                    isAdvertising = false
                    Log.e(TAG, "Advertising failed: errorCode=$errorCode")
                }
            }

            adv.startAdvertising(settings, data, advertiseCallback)
        } catch (e: Exception) {
            Log.e(TAG, "startAdvertising exception: ${e.message}", e)
        }
    }

    fun stopAdvertising() {
        try {
            advertiser?.stopAdvertising(advertiseCallback)
        } catch (e: Exception) {
            Log.e(TAG, "stopAdvertising exception: ${e.message}")
        }
        advertiseCallback = null
        isAdvertising = false
        Log.i(TAG, "Advertising stopped")
    }

    // --- Existing API ---

    fun isPaired(): Boolean = isPairedState

    fun getDeviceName(): String = pairedDeviceName ?: "Not connected"

    fun getLastRssi(): Int = lastRssi

    fun sendUnlockChallenge() {
        Log.i(TAG, "sendUnlockChallenge called")
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

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02X".format(it) }
}
