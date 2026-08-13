const { invoke } = window.__TAURI__;

// Navigation
document.querySelectorAll('.nav-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.add('hidden'));
    document.getElementById('tab-' + btn.dataset.tab).classList.remove('hidden');
    document.querySelectorAll('.nav-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
  });
});

// Status polling
async function refreshStatus() {
  try {
    const st = await invoke('get_status');
    const icon = document.getElementById('statusIcon');
    const title = document.getElementById('statusTitle');
    const detail = document.getElementById('statusDetail');
    const daemonToggle = document.getElementById('daemonToggle');

    title.textContent = st.state.charAt(0).toUpperCase() + st.state.slice(1);
    detail.textContent = st.detail;

    if (st.state === 'locked') {
      icon.textContent = '🔒';
      icon.classList.add('locked');
    } else if (st.state === 'authenticated') {
      icon.textContent = '🔓';
      icon.classList.remove('locked');
    } else {
      icon.textContent = '⏳';
      icon.classList.remove('locked');
    }

    if (daemonToggle) daemonToggle.checked = st.daemon_enabled;
  } catch (e) {
    console.error('Status error:', e);
  }
}
setInterval(refreshStatus, 2000);
refreshStatus();

// Daemon toggle
document.getElementById('daemonToggle')?.addEventListener('change', async (e) => {
  try {
    await invoke('toggle_daemon', { enabled: e.target.checked });
  } catch (err) {
    console.error('Toggle daemon error:', err);
  }
});

// Paired devices
document.getElementById('tab-devices').addEventListener('click', loadDevices);
async function loadDevices() {
  try {
    const devices = await invoke('get_paired_devices');
    const list = document.getElementById('pairedList');
    if (!devices.length) {
      list.innerHTML = '<p class="muted">No paired devices yet.</p>';
      return;
    }
    list.innerHTML = devices.map(d => `
      <div class="device-row">
        <div class="device-info">
          <span class="device-icon">⌚</span>
          <div>
            <div class="device-name">${escapeHtml(d.name)}</div>
            <div class="device-meta">Baseline ${d.baseline_rssi} dBm • ${d.address}</div>
          </div>
        </div>
        <div>
          <button class="btn btn-primary" style="margin-right:8px" onclick="calibrateDevice('${d.id}', '${escapeHtml(d.name)}')">📏 Calibrate</button>
          <button class="btn btn-danger" onclick="forgetDevice('${d.id}')">Forget</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.error('Devices error:', e);
  }
}

// Scan
document.getElementById('scanBtn').addEventListener('click', async () => {
  const btn = document.getElementById('scanBtn');
  const res = document.getElementById('scanResults');
  btn.disabled = true;
  btn.textContent = '⏳ Scanning…';
  res.innerHTML = '<p class="muted">Looking for WristKey devices…</p>';
  try {
    const found = await invoke('scan_devices');
    if (!found.length) {
      res.innerHTML = '<p class="muted">No devices found. Make sure watch app is open.</p>';
    } else {
      res.innerHTML = found.map(d => `
        <div class="device-row" style="background:rgba(59,130,246,0.08)">
          <div class="device-info">
            <span class="device-icon">⌚</span>
            <div>
              <div class="device-name">${escapeHtml(d.name)}</div>
              <div class="device-meta">RSSI: ${d.rssi} dBm</div>
            </div>
          </div>
          <button class="btn btn-primary" onclick="pairDevice('${escapeHtml(d.id)}', '${escapeHtml(d.name)}', ${d.rssi})">🔗 Pair</button>
        </div>
      `).join('');
    }
  } catch (e) {
    res.innerHTML = '<p class="muted">Scan error: ' + escapeHtml(e.toString()) + '</p>';
  }
  btn.disabled = false;
  btn.textContent = '🔍 Scan for 30 seconds';
});

// Settings
document.getElementById('saveBtn').addEventListener('click', async () => {
  const cfg = {
    auto_lock_timeout_sec: parseInt(document.getElementById('timeoutInput').value),
    rssi_threshold_offset_dbm: parseInt(document.getElementById('rssiOffsetInput').value),
    challenge_timeout_sec: parseInt(document.getElementById('challengeInput').value),
  };
  try {
    await invoke('set_config', { config: cfg });
    document.getElementById('saveStatus').textContent = '✅ Saved';
    setTimeout(() => document.getElementById('saveStatus').textContent = '', 2000);
  } catch (e) {
    document.getElementById('saveStatus').textContent = '❌ ' + e;
  }
});

// Lock button
document.getElementById('lockBtn').addEventListener('click', async () => {
  try {
    await invoke('lock_screen');
  } catch (e) {
    alert('Lock failed: ' + e);
  }
});

// Unlock button
document.getElementById('unlockBtn')?.addEventListener('click', async () => {
  try {
    await invoke('unlock_screen');
  } catch (e) {
    alert('Unlock failed: ' + e);
  }
});

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

async function forgetDevice(id) {
  if (!confirm('Remove this device?')) return;
  try {
    await invoke('forget_device', { id });
    loadDevices();
  } catch (e) {
    alert('Forget failed: ' + e);
  }
}

async function pairDevice(id, name, rssi) {
  if (!confirm(`Pair with ${name}?`)) return;
  const btn = event.target;
  btn.disabled = true;
  btn.textContent = '⏳ Pairing…';
  try {
    await invoke('pair_device', { req: { id, name, rssi } });
    alert('✅ Paired successfully!');
    loadDevices();
  } catch (e) {
    alert('❌ Pair failed: ' + e);
  }
  btn.disabled = false;
  btn.textContent = '🔗 Pair';
}

async function calibrateDevice(id, name) {
  if (!confirm(`Calibrate proximity for ${name}?\nHold watch near PC for 10s.`)) return;

  const progressDiv = document.getElementById('calibrateProgress');
  const progressBar = document.getElementById('calibrateBar');
  const progressText = document.getElementById('calibrateText');
  progressDiv.classList.remove('hidden');

  let progress = 0;
  const interval = setInterval(() => {
    progress += 10;
    progressBar.style.width = progress + '%';
    progressText.textContent = `Calibrating… ${progress}%`;
    if (progress >= 100) clearInterval(interval);
  }, 1000);

  try {
    const result = await invoke('calibrate_proximity', { id });
    clearInterval(interval);
    progressBar.style.width = '100%';
    progressText.textContent = `✅ Calibrated! Avg: ${result.avg} dBm, Threshold: ${result.threshold} dBm`;
    setTimeout(() => progressDiv.classList.add('hidden'), 3000);
    loadDevices();
  } catch (e) {
    clearInterval(interval);
    progressText.textContent = '❌ Calibration failed: ' + e;
    setTimeout(() => progressDiv.classList.add('hidden'), 3000);
  }
}
