package com.wristkey.app

import android.app.Application
import android.util.Log

class WristKeyApp : Application() {
    companion object {
        private const val TAG = "WristKeyApp"
    }

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "WristKey application started")
    }
}
