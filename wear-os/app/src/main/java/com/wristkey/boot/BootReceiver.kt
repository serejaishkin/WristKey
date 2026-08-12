package com.wristkey.boot

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import com.wristkey.ble.WristKeyBleService

/**
 * Restarts the BLE service after the watch reboots.
 *
 * Wear OS watches (Galaxy Watch 4 included) reboot on their own for system
 * updates, and Samsung's power management can fully stop the app process
 * rather than just killing the service. Without this, unlock silently stops
 * working after any reboot until the person notices and manually reopens
 * the WristKey app on the watch.
 */
class BootReceiver : BroadcastReceiver() {
    companion object {
        private const val TAG = "WristKeyBootReceiver"
    }

    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != Intent.ACTION_BOOT_COMPLETED &&
            intent?.action != "android.intent.action.QUICKBOOT_POWERON") {
            return
        }
        Log.i(TAG, "Boot completed, restarting WristKey BLE service")
        val serviceIntent = Intent(context, WristKeyBleService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(serviceIntent)
        } else {
            context.startService(serviceIntent)
        }
    }
}
