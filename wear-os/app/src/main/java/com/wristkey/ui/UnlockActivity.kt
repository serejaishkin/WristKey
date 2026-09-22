package com.wristkey.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.*
import com.wristkey.R
import com.wristkey.security.TouchPointStore

class UnlockActivity : ComponentActivity() {

    private var decided = false
    private val trainedVersion = mutableStateOf(0)

    private val trainingLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) trainedVersion.value++
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val user = intent.getStringExtra("user") ?: getString(R.string.default_user)
        setContent {
            MaterialTheme {
                UnlockGateScreen(
                    user = user,
                    trainedVersion = trainedVersion.value,
                    onTrainRequested = { trainingLauncher.launch(Intent(this, TrainingActivity::class.java)) },
                    onApprove = { decide(true) },
                    onCancel = { decide(false) }
                )
            }
        }
    }

    private fun decide(approved: Boolean) {
        if (decided) return
        decided = true
        sendUnlockBroadcast(approved)
        finish()
    }

    override fun onDestroy() {
        if (!decided) {
            decided = true
            sendUnlockBroadcast(approved = false)
        }
        super.onDestroy()
    }

    private fun sendUnlockBroadcast(approved: Boolean) {
        sendBroadcast(Intent(ACTION_UNLOCK).apply {
            setPackage(packageName)
            putExtra(EXTRA_APPROVED, approved)
        })
    }

    companion object {
        const val ACTION_UNLOCK = "com.wristkey.UNLOCK_ACTION"
        const val EXTRA_APPROVED = "approved"
    }
}

@Composable
fun UnlockGateScreen(
    user: String,
    trainedVersion: Int,
    onTrainRequested: () -> Unit,
    onApprove: () -> Unit,
    onCancel: () -> Unit
) {
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }

    if (!store.isTrained()) {
        NoPointScreen(user, onTrainRequested, onCancel)
    } else {
        ZoneApprovalScreen(user, onApprove, onCancel)
    }
}

@Composable
fun NoPointScreen(user: String, onTrainRequested: () -> Unit, onCancel: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(stringResource(R.string.title_unlock), style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
        Text(user, style = MaterialTheme.typography.caption2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(10.dp))
        Text(stringResource(R.string.unlock_point_not_set), style = MaterialTheme.typography.caption1, textAlign = TextAlign.Center, color = Color(0xFFFFB74D))
        Spacer(Modifier.height(12.dp))
        Button(onClick = onTrainRequested, modifier = Modifier.fillMaxWidth(0.8f)) { Text(stringResource(R.string.btn_train_point_unlock)) }
        Spacer(Modifier.height(6.dp))
        Button(onClick = onCancel, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.8f)) { Text(stringResource(R.string.btn_cancel_unlock)) }
    }
}

@Composable
fun ZoneApprovalScreen(user: String, onApprove: () -> Unit, onCancel: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(stringResource(R.string.title_unlock), style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
        Text(user, style = MaterialTheme.typography.caption2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(10.dp))
        Text(stringResource(R.string.unlock_zone_instruction), style = MaterialTheme.typography.body1, textAlign = TextAlign.Center)
        Spacer(Modifier.height(12.dp))
        Button(onClick = onApprove, modifier = Modifier.fillMaxWidth(0.7f)) { Text(stringResource(R.string.btn_approve_unlock)) }
        Spacer(Modifier.height(6.dp))
        Button(onClick = onCancel, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.7f)) { Text(stringResource(R.string.btn_cancel_unlock)) }
    }
}