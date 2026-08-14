/* WristKey Desktop Frontend */

let currentState = 'disconnected';
let currentDeviceCount = 0;
let daemonEnabled = false;
let cpRegistered = false;

async function invoke(cmd, args = {}) {
  try {
    return await window.__TAURI__.core.invoke(cmd, args);
  } catch (e) {
    console.error(`Invoke ${cmd} failed:`, e);
    throw e;
  }
}

async function refreshStatus() {
  try {
    const status = await invoke('get_status');
    currentState = status.state;
    currentDeviceCount = status.device_count;
    daemonEnabled = status.daemon_enabled;
    cpRegistered = status.cp_registered || false;

    document.getElementById('statusState').textContent = status.detail;
    document.getElementById('statusDot').className = 'status-dot ' + status.state;
    document.getElementById('deviceCount').textContent = status.device_count;
    document.getElementById('daemonToggle').checked = status.daemon_enabled;

    // Windows CP tab visibility
    const cpTab = document.getElementById('tab-cp');
    if (cpTab) {
      cpTab.style.display = 'block';
    }
    const cpStatus = document.getElementById('cpStatus');
    if (cpStatus) {
      cpStatus.textContent = cpRegistered ? '✅ Registered' : '❌ Not registered';
      cpStatus.className = cpRegistered ? 'status-ok' : 'status-warn';
    }
    const cpBtn = document.getElementById('cpRegisterBtn');
    if (cpBtn) {
      cpBtn.textContent = cpRegistered ? '🔁 Re-register CP' : '📝 Register Credential Provider';
    }
  } catch (e) {
    document.getElementById('statusState').textContent = 'Error: ' + e;
  }
}

async function refreshDevices() {
  try {
    const devices = await invoke('get_paired_devices');
    const list = document.getElementById('pairedList');
    list.innerHTML = '';
    if (devices.length === 0) {
      list.innerHTML = '<div class="empty">No paired devices. Scan to pair.</div>';
    } else {
      devices.forEach(d => {
        const div = document.createElement('div');
        div.className = 'device-card';
        div.innerHTML = `<strong>${d.name}</strong><br><small>${d.address} • RSSI baseline: ${d.baseline_rssi} dBm</small><br>
          <button onclick="forgetDevice('${d.id}')">🗑 Forget</button>
          <button onclick="calibrateDevice('${d.id}')">📡 Calibrate</button>`;
        list.appendChild(div);
      });
    }
  } catch (e) {
    console.error('refreshDevices failed:', e);
  }
}

async function scanDevices() {
  const btn = document.getElementById('scanBtn');
  btn.disabled = true;
  btn.textContent = '🔍 Scanning...';
  try {
    const found = await invoke('scan_devices');
    const list = document.getElementById('scanList');
    list.innerHTML = '';
    if (found.length === 0) {
      list.innerHTML = '<div class="empty">No devices found. Make sure watch app is open.</div>';
    } else {
      found.forEach(d => {
        const div = document.createElement('div');
        div.className = 'device-card';
        div.innerHTML = `<strong>${d.name}</strong><br><small>RSSI: ${d.rssi} dBm • ${d.address}</small><br>
          <button onclick="pairDevice('${d.id}', '${d.name}', ${d.rssi}, '${d.address}')">🔗 Pair</button>`;
        list.appendChild(div);
      });
    }
  } catch (e) {
    alert('Scan failed: ' + e);
  } finally {
    btn.disabled = false;
    btn.textContent = '🔍 Scan for New Device';
  }
}

async function pairDevice(id, name, rssi, address) {
  try {
    await invoke('pair_device', { req: { id, name, rssi, address } });
    alert('✅ Paired successfully!');
    await refreshDevices();
    await refreshStatus();
  } catch (e) {
    alert('❌ Pairing failed: ' + e);
  }
}

async function forgetDevice(id) {
  if (!confirm('Forget this device?')) return;
  try {
    await invoke('forget_device', { id });
    await refreshDevices();
    await refreshStatus();
  } catch (e) {
    alert('Forget failed: ' + e);
  }
}

async function calibrateDevice(id) {
  const btn = event.target;
  btn.disabled = true;
  btn.textContent = '⏳ Calibrating...';
  try {
    const result = await invoke('calibrate_proximity', { id });
    alert(`📡 Calibration complete!\nMedian: ${result.avg} dBm\nThreshold: ${result.threshold} dBm\nSamples: ${result.samples}`);
  } catch (e) {
    alert('Calibration failed: ' + e);
  } finally {
    btn.disabled = false;
    btn.textContent = '📡 Calibrate';
  }
}

async function loadConfig() {
  try {
    const cfg = await invoke('get_config');
    document.getElementById('timeoutInput').value = cfg.auto_lock_timeout_sec;
    document.getElementById('rssiInput').value = cfg.rssi_threshold_offset_dbm;
    document.getElementById('challengeInput').value = cfg.challenge_timeout_sec;
    document.getElementById('logFileToggle').checked = cfg.log_to_file;
    document.getElementById('logConsoleToggle').checked = cfg.log_to_console;
    document.getElementById('logLevelSelect').value = cfg.log_level;
  } catch (e) {
    console.error('loadConfig failed:', e);
  }
}

async function saveConfig() {
  try {
    const cfg = {
      auto_lock_timeout_sec: parseInt(document.getElementById('timeoutInput').value),
      rssi_threshold_offset_dbm: parseInt(document.getElementById('rssiInput').value),
      challenge_timeout_sec: parseInt(document.getElementById('challengeInput').value),
      log_to_file: document.getElementById('logFileToggle').checked,
      log_to_console: document.getElementById('logConsoleToggle').checked,
      log_level: document.getElementById('logLevelSelect').value,
    };
    await invoke('set_config', { config: cfg });
    alert('💾 Saved!');
  } catch (e) {
    alert('❌ Save failed: ' + e);
  }
}

async function toggleDaemon() {
  const enabled = document.getElementById('daemonToggle').checked;
  try {
    await invoke('toggle_daemon', { enabled });
    daemonEnabled = enabled;
  } catch (e) {
    alert('Toggle daemon failed: ' + e);
    document.getElementById('daemonToggle').checked = !enabled;
  }
}

async function lockNow() {
  try {
    await invoke('lock_screen');
  } catch (e) {
    alert('Lock failed: ' + e);
  }
}

async function setWindowsPassword() {
  const pwd = document.getElementById('winPasswordInput').value;
  if (!pwd) {
    alert('Enter your Windows password');
    return;
  }
  try {
    await invoke('set_windows_password', { password: pwd });
    alert('💾 Windows password encrypted and saved!');
    document.getElementById('winPasswordInput').value = '';
  } catch (e) {
    alert('❌ Failed: ' + e);
  }
}

async function registerCP() {
  try {
    await invoke('register_credential_provider');
    alert('✅ Credential Provider registered! Restart PC to apply.');
    await refreshStatus();
  } catch (e) {
    alert('❌ Registration failed: ' + e);
  }
}

async function unregisterCP() {
  if (!confirm('Unregister Credential Provider?')) return;
  try {
    await invoke('unregister_credential_provider');
    alert('✅ Unregistered. Restart PC to apply.');
    await refreshStatus();
  } catch (e) {
    alert('❌ Failed: ' + e);
  }
}

// Tab switching
function showTab(tab) {
  document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));
  document.querySelectorAll('.tab-btn').forEach(el => el.classList.remove('active'));
  document.getElementById('tab-' + tab).classList.add('active');
  document.querySelector(`[data-tab="${tab}"]`).classList.add('active');
}

document.addEventListener('DOMContentLoaded', () => {
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.addEventListener('click', () => showTab(btn.dataset.tab));
  });

  document.getElementById('daemonToggle').addEventListener('change', toggleDaemon);
  document.getElementById('scanBtn').addEventListener('click', scanDevices);
  document.getElementById('saveConfigBtn').addEventListener('click', saveConfig);
  document.getElementById('lockNowBtn').addEventListener('click', lockNow);

  const cpBtn = document.getElementById('cpRegisterBtn');
  if (cpBtn) cpBtn.addEventListener('click', registerCP);
  const cpUnregBtn = document.getElementById('cpUnregisterBtn');
  if (cpUnregBtn) cpUnregBtn.addEventListener('click', unregisterCP);
  const winPwdBtn = document.getElementById('winPasswordBtn');
  if (winPwdBtn) winPwdBtn.addEventListener('click', setWindowsPassword);

  refreshStatus();
  refreshDevices();
  loadConfig();
  setInterval(refreshStatus, 3000);
});
