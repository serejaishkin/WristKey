package com.wristkey.ble

import android.app.*
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.*
import android.os.*
import android.util.*
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.security.KeyStoreManager
import com.wristkey.ui.PairingActivity
import com.wristkey.ui.UnlockActivity
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

class WristKeyBleService : Service() {
    companion object {
        private const val TAG = "WristKeyBleService"
        private const val DEBUG_TAG = "WristKeyBLE"
        private const val NOTIFICATION_ID = 1
        private const val CHANNEL_ID = "wristkey_ble_channel"
        private const val PREFS_NAME = "WristKeyPrefs"
        private const val PREFS_PAIRED_ADDRESS = "paired_device_address"
        private const val PREFS_PAIRED_NAME = "paired_device_name"
        private const val PREFS_PAIRING_KEY = "pairing_key"
        private val CCCD_UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        val SERVICE_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val PUBLIC_KEY_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CONFIG_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
        val UNLOCK_REQUEST_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567895")
        val UNLOCK_RESPONSE_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567896")
        val PAIRING_KEY_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567897")
        val PC_NAME_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567898")
    }

    private val binder = LocalBinder()
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var challengeCharacteristic: BluetoothGattCharacteristic? = null
    private var publicKeyCharacteristic: BluetoothGattCharacteristic? = null
    private var configCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockRequestCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockResponseCharacteristic: BluetoothGattCharacteristic? = null
    private var pairingKeyCharacteristic: BluetoothGattCharacteristic? = null
    private var pcNameCharacteristic: BluetoothGattCharacteristic? = null
    private var connectedDevice: BluetoothDevice? = null
    private var pairingDeviceAddress: String? = null
    private var requestingPcName: String? = null
    private var currentChallenge: ByteArray? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private val keyStoreManager = KeyStoreManager()
    private val prefs by lazy { getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE) }
    private var pairedDeviceAddress: String? = null
    private var pairedDeviceName: String? = null
    private var currentPin = (1000..9999).random()
    private var lastRssi = 0
    private var previousRssi: Int? = null
    private val proximityTracker = ProximityRssiTracker()
    private var proximityState = ProximityRssiTracker.State.UNKNOWN
    private var knownDeviceConnected = false
    private val _pairingRequested = AtomicBoolean(false)
    val pairingRequested get() = _pairingRequested
    private val _userPresent = AtomicBoolean(false)
    val userPresent get() = _userPresent
    private val _userPresentCountdown = AtomicInteger(0)
    val userPresentCountdown get() = _userPresentCountdown

    inner class LocalBinder : Binder() { fun getService() = this@WristKeyBleService }
    private fun debug(message: String) = Log.i(DEBUG_TAG, message)

    override fun onCreate() {
        super.onCreate()
        pairedDeviceAddress = prefs.getString(PREFS_PAIRED_ADDRESS, null)
        pairedDeviceName = prefs.getString(PREFS_PAIRED_NAME, null)
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = manager.adapter
        if (bluetoothAdapter == null) { stopSelf(); return }
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "WristKey::BleWakeLock")
        wakeLock?.acquire(10 * 60 * 1000L)
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())
        startGattServer(); startAdvertising(); registerBluetoothStateReceiver(); registerUnlockReceiver()
        debug("BLE service created; paired=${isPaired()} advertising=${isAdvertising()}")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        wakeLock?.let { if (!it.isHeld) it.acquire(10 * 60 * 1000L) }
        if (gattServer == null) startGattServer()
        if (advertiseCallback == null) startAdvertising()
        return START_STICKY
    }

    override fun onBind(intent: Intent): IBinder = binder

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "WristKey BLE", NotificationManager.IMPORTANCE_LOW)
            )
        }
    }
    private fun buildNotification() = NotificationCompat.Builder(this, CHANNEL_ID)
        .setContentTitle("WristKey")
        .setContentText(if (isPaired()) "Paired: ${getPairedDeviceAddress()}" else "BLE advertising active -- PIN: ${getAdvertisePin()}")
        .setSmallIcon(R.drawable.ic_launcher).setOngoing(true).build()
    fun updateNotification() = getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    fun getAdvertisePin() = String.format("%04d", currentPin)
    fun getPairedDeviceAddress() = pairedDeviceAddress
    fun getCurrentBluetoothAddress() = bluetoothAdapter?.address
    fun getRequestingPcName() = requestingPcName
    fun getPairedDeviceName() = pairedDeviceName
    fun getPairingDeviceAddress() = pairingDeviceAddress ?: "--"
    fun getCurrentChallengeSize() = currentChallenge?.size ?: 0
    fun hasPendingPairing() = pairingDeviceAddress != null && currentChallenge != null
    fun getLastRssi() = lastRssi
    fun getProximityState() = proximityState.name
    fun getFilteredRssi(): Double? = proximityTracker.snapshot().filteredRssi
    fun isPaired() = pairedDeviceAddress != null
    fun isAdvertising() = advertiseCallback != null
    fun getPairedDeviceCount() = if (isPaired()) 1 else 0
    fun getConnectedDeviceAddress() = connectedDevice?.address ?: "--"
    fun getDeviceName() = pairedDeviceName ?: pairedDeviceAddress ?: "Unknown"

    fun setPairedDevice(address: String, name: String) {
        pairedDeviceAddress = address; pairedDeviceName = name
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).putString(PREFS_PAIRED_NAME, name).apply()
        updateNotification()
    }
    fun setPairedDeviceAddress(address: String) { pairedDeviceAddress = address; prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).apply() }
    fun clearPairedDevice() { pairedDeviceAddress = null; pairedDeviceName = null; prefs.edit().remove(PREFS_PAIRED_ADDRESS).remove(PREFS_PAIRED_NAME).remove(PREFS_PAIRING_KEY).apply(); proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; updateNotification() }
    fun forgetDevice() { clearPairedDevice(); stopGattServer(); resetPin(); startGattServer(); startAdvertising() }
    fun resetPin() { currentPin = (1000..9999).random(); updateNotification() }
    fun rejectPairing() { _pairingRequested.set(false); currentChallenge = null; pairingDeviceAddress = null; requestingPcName = null }

    private fun showPairingActivity() {
        if (isPaired()) return
        try { startActivity(Intent(this, PairingActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP; putExtra("pcName", requestingPcName ?: "Windows PC"); putExtra("pcAddress", pairingDeviceAddress ?: "") }) }
        catch (e: Exception) { Log.e(TAG, "Failed to open pairing UI", e) }
    }

    fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        if (advertiseCallback != null || !adapter.isEnabled) return
        val advertiser = adapter.bluetoothLeAdvertiser ?: return
        val data = AdvertiseData.Builder().addServiceUuid(ParcelUuid(SERVICE_UUID)).setIncludeDeviceName(false).addManufacturerData(0xFFFF, getAdvertisePin().toByteArray()).build()
        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) { debug("Advertising started pin=${getAdvertisePin()}") }
            override fun onStartFailure(errorCode: Int) { Log.e(TAG, "Advertising failed: $errorCode"); advertiseCallback = null }
        }
        advertiser.startAdvertising(AdvertiseSettings.Builder().setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY).setConnectable(true).setTimeout(0).setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH).build(), data, AdvertiseData.Builder().setIncludeDeviceName(true).build(), callback)
        advertiseCallback = callback
    }
    fun stopAdvertising() { advertiseCallback?.let { bluetoothAdapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }; advertiseCallback = null }
    private fun cccd() = BluetoothGattDescriptor(CCCD_UUID, BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE)

    private fun startGattServer() {
        if (gattServer != null) return
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        gattServer = manager.openGattServer(this, gattServerCallback) ?: return
        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        challengeCharacteristic = BluetoothGattCharacteristic(CHALLENGE_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        responseCharacteristic = BluetoothGattCharacteristic(RESPONSE_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ).also { it.addDescriptor(cccd()) }
        publicKeyCharacteristic = BluetoothGattCharacteristic(PUBLIC_KEY_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ)
        configCharacteristic = BluetoothGattCharacteristic(CONFIG_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_READ or BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockRequestCharacteristic = BluetoothGattCharacteristic(UNLOCK_REQUEST_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockResponseCharacteristic = BluetoothGattCharacteristic(UNLOCK_RESPONSE_UUID, BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ).also { it.addDescriptor(cccd()) }
        pairingKeyCharacteristic = BluetoothGattCharacteristic(PAIRING_KEY_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        pcNameCharacteristic = BluetoothGattCharacteristic(PC_NAME_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        listOf(challengeCharacteristic, responseCharacteristic, publicKeyCharacteristic, configCharacteristic, unlockRequestCharacteristic, unlockResponseCharacteristic, pairingKeyCharacteristic, pcNameCharacteristic).forEach { service.addCharacteristic(it) }
        if (gattServer?.addService(service) != true) Log.e(TAG, "GATT addService failed")
    }

    private fun stopGattServer() {
        gattServer?.close(); gattServer = null; connectedDevice = null
        challengeCharacteristic = null; responseCharacteristic = null; publicKeyCharacteristic = null; configCharacteristic = null; unlockRequestCharacteristic = null; unlockResponseCharacteristic = null; pairingKeyCharacteristic = null; pcNameCharacteristic = null
        proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; previousRssi = null
    }

    private fun updateProximity(device: BluetoothDevice?, rssi: Int) {
        if (device == null || !isPaired() || device.address != pairedDeviceAddress) return
        lastRssi = rssi
        val abrupt = previousRssi?.let { ProximityRssiTracker.isAbruptChange(it, rssi) } ?: false
        previousRssi = rssi
        val snapshot = proximityTracker.update(rssi)
        proximityState = snapshot.state
        debug("RSSI address=${device.address} raw=$rssi filtered=${"%.1f".format(snapshot.filteredRssi)} state=${snapshot.state} abrupt=$abrupt")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            if (device == null) return
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    connectedDevice = device; pairingDeviceAddress = device.address
                    knownDeviceConnected = isPaired() && device.address == pairedDeviceAddress
                    debug("PC connected address=${device.address} paired=$knownDeviceConnected")
                    if (!knownDeviceConnected) { _pairingRequested.set(true); showPairingActivity() }
                    else { _pairingRequested.set(false); currentChallenge = null; proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; previousRssi = null; debug("Known device reconnected; pairing UI suppressed") }
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    if (connectedDevice?.address == device.address) connectedDevice = null
                    if (device.address == pairedDeviceAddress) {
                        knownDeviceConnected = false
                        proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; previousRssi = null
                        debug("Known device disconnected; proximity reset, pairing retained")
                    }
                    if (pairingDeviceAddress == device.address) pairingDeviceAddress = null
                    _pairingRequested.set(false)
                }
            }
        }
        override fun onDescriptorWriteRequest(device: BluetoothDevice?, requestId: Int, descriptor: BluetoothGattDescriptor?, preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?) {
            if (descriptor?.uuid != CCCD_UUID) { if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null); return }
            descriptor.value = value
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
        }
        override fun onCharacteristicWriteRequest(device: BluetoothDevice?, requestId: Int, characteristic: BluetoothGattCharacteristic?, preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?) {
            when (characteristic?.uuid) {
                CHALLENGE_CHAR_UUID -> { currentChallenge = value; pairingDeviceAddress = device?.address ?: pairingDeviceAddress; connectedDevice = device ?: connectedDevice; if (!isPaired()) { _pairingRequested.set(true); showPairingActivity() } else debug("Challenge received from known paired device; pairing UI suppressed") }
                CONFIG_CHAR_UUID -> debug("Config write bytes=${value?.size ?: 0}")
                UNLOCK_REQUEST_UUID -> handleUnlockRequest(value)
                PAIRING_KEY_CHAR_UUID -> value?.let { setPairingKey(it) }
                PC_NAME_CHAR_UUID -> value?.let { requestingPcName = String(it, Charsets.UTF_8).trim('\u0000') }
                else -> { if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null); return }
            }
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
        }
        override fun onCharacteristicReadRequest(device: BluetoothDevice?, requestId: Int, offset: Int, characteristic: BluetoothGattCharacteristic?) {
            when (characteristic?.uuid) {
                PUBLIC_KEY_CHAR_UUID -> try { val key = keyStoreManager.getPublicKey(); if (offset >= key.size) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_OFFSET, offset, null) else gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, key.copyOfRange(offset, minOf(offset + 512, key.size))) }
                catch (e: Exception) { gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null) }
                CONFIG_CHAR_UUID -> { val data = byteArrayOf(1, (currentPin shr 8).toByte(), currentPin.toByte(), if (isPaired()) 1 else 0); if (offset >= data.size) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_OFFSET, offset, null) else gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, data.copyOfRange(offset, data.size)) }
                else -> gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
            }
        }
    }

    fun confirmPairing(): Boolean = try {
        val challenge = currentChallenge ?: return false
        val pc = connectedDevice ?: pairingDeviceAddress?.let { bluetoothAdapter?.getRemoteDevice(it) } ?: return false
        val signature = keyStoreManager.signChallenge(challenge)
        responseCharacteristic?.value = signature + byteArrayOf(1)
        setPairedDevice(pc.address, requestingPcName ?: "Windows PC")
        setPairingKey(UnlockProtocol.generatePasswordKey())
        _userPresent.set(true); _pairingRequested.set(false)
        proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; previousRssi = null
        gattServer?.notifyCharacteristicChanged(pc, responseCharacteristic, false)
        debug("Pairing confirmed for ${pc.address}; pairing persisted")
        true
    } catch (e: Exception) { Log.e(TAG, "Pairing confirmation failed", e); false }

    fun requestUserPresence(): Boolean { if (_userPresent.get()) return true; _userPresentCountdown.set(10); return false }
    fun setUserPresent(present: Boolean) { _userPresent.set(present) }

    private fun handleUnlockRequest(data: ByteArray?) {
        if (data == null) return
        val pairingKey = getPairingKey() ?: run { sendUnlockResponse(null, "NO_PAIRING_KEY"); return }
        try { val request = JSONObject(String(UnlockProtocol.decrypt(data, pairingKey))); val user = request.optString("user", "Unknown PC"); startActivity(Intent(this, UnlockActivity::class.java).apply { putExtra("user", user); flags = Intent.FLAG_ACTIVITY_NEW_TASK }) }
        catch (e: Exception) { Log.e(TAG, "Unlock request failed", e); sendUnlockResponse(null, "DECRYPT_ERROR") }
    }
    private fun sendUnlockResponse(passwordKey: ByteArray?, error: String? = null) {
        val key = getPairingKey() ?: return
        val response = JSONObject().apply { put("token", "wristkey_unlock"); if (passwordKey != null) put("password_key", Base64.encodeToString(passwordKey, Base64.NO_WRAP)) else put("error", error ?: "UNKNOWN") }
        try { unlockResponseCharacteristic?.value = UnlockProtocol.encrypt(response.toString().toByteArray(), key); connectedDevice?.let { gattServer?.notifyCharacteristicChanged(it, unlockResponseCharacteristic, false) } } catch (e: Exception) { Log.e(TAG, "Unlock response failed", e) }
    }
    private fun getPairingKey() = prefs.getString(PREFS_PAIRING_KEY, null)?.let { Base64.decode(it, Base64.DEFAULT) }
    private fun setPairingKey(key: ByteArray) { prefs.edit().putString(PREFS_PAIRING_KEY, Base64.encodeToString(key, Base64.DEFAULT)).apply() }

    private fun registerUnlockReceiver() {
        val filter = IntentFilter("com.wristkey.UNLOCK_ACTION")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) registerReceiver(unlockReceiver, filter, Context.RECEIVER_NOT_EXPORTED) else registerReceiver(unlockReceiver, filter)
    }
    private val unlockReceiver = object : BroadcastReceiver() { override fun onReceive(context: Context?, intent: Intent?) { if (intent?.getBooleanExtra("approved", false) == true) sendUnlockResponse(getPairingKey(), null) else sendUnlockResponse(null, "CANCEL") } }
    private fun registerBluetoothStateReceiver() {
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) registerReceiver(bluetoothStateReceiver, filter, Context.RECEIVER_NOT_EXPORTED) else registerReceiver(bluetoothStateReceiver, filter)
    }
    private val bluetoothStateReceiver = object : BroadcastReceiver() { override fun onReceive(context: Context?, intent: Intent?) { when (intent?.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) { BluetoothAdapter.STATE_OFF -> { stopAdvertising(); stopGattServer() }; BluetoothAdapter.STATE_ON -> { startGattServer(); startAdvertising() } } } }
    override fun onDestroy() { stopAdvertising(); stopGattServer(); try { unregisterReceiver(bluetoothStateReceiver) } catch (_: Exception) {}; try { unregisterReceiver(unlockReceiver) } catch (_: Exception) {}; wakeLock?.let { if (it.isHeld) it.release() }; super.onDestroy() }
}
