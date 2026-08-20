/* WristKey Desktop Frontend */

let currentState = 'disconnected';
let currentDeviceCount = 0;
let daemonEnabled = false;
let cpRegistered = false;
let calibrationListenersReady = false;

async function invoke(cmd, args = {}) {
    try {
        return await window.__TAURI__.core.invoke(cmd, args);
    } catch (e) {
        console.error(`Invoke ${cmd} failed:`, e);
        throw e;
    }
}

function updateCalibrationProgress(deviceId, current, total, rssi) {
    const percent = total > 0 ? Math.round((current / total) * 100) : 0;
    const cards = document.querySelectorAll(`[data-calibration-device="${CSS.escape(deviceId)}"]`);
    cards.forEach(card => {
        const progress = card.querySelector('.calibration-progress');
        const text = card.querySelector('.calibration-progress-text');
        if (progress) progress.value = percent;
        if (text) text.textContent = `⏳ ${current}/${total} • ${rssi} dBm`;
    });
}

async function setupCalibrationListeners() {
    if (calibrationListenersReady || !window.__TAURI__?.event) return;
    calibrationListenersReady = true;
    await window.__TAURI__.event.listen('calibration-progress', event => {
        const p = event.payload || {};
        updateCalibrationProgress(String(p.device_id ?? ''), Number(p.current ?? 0), Number(p.total ?? 0), Number(p.rssi ?? -100));
    });
    await window.__TAURI__.event.listen('calibration-done', event => {
        const p = event.payload || {};
        const cards = document.querySelectorAll(`[data-calibration-device="${CSS.escape(String(p.device_id ?? ''))}"]`);
        cards.forEach(card => {
            const progress = card.querySelector('.calibration-progress');
            const text = card.querySelector('.calibration-progress-text');
            if (progress) progress.value = 100;
            if (text) text.textContent = `✅ ${Number(p.samples ?? 0)} samples • median ${Number(p.median ?? 0)} dBm • threshold ${Number(p.threshold ?? 0)} dBm`;
        });
    });
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

        const cpTabBtn = document.getElementById('tab-btn-cp');
        if (cpTabBtn) cpTabBtn.classList.remove('hidden');
        const cpStatus = document.getElementById('cpStatus');
        if (cpStatus) {
            cpStatus.textContent = cpRegistered ? '✅ Registered' : '❌ Not registered';
            cpStatus.className = cpRegistered ? 'status-ok' : 'status-warn';
        }
        const cpBtn = document.getElementById('cpRegisterBtn');
        if (cpBtn) cpBtn.textContent = cpRegistered ? '🔁 Re-register CP' : '📝 Register Credential Provider';

        const storageType = document.getElementById('storageType');
        if (storageType && status.storage_type) {
            storageType.textContent = status.storage_type;
            if (status.storage_type.includes('TPM')) storageType.innerHTML = '🔒 TPM 2.0';
            else if (status.storage_type.includes('Software')) storageType.innerHTML = '💻 Software';
        }
    } catch (e) {
        const el = document.getElementById('statusState');
        if (el) el.textContent = 'Error: ' + e;
    }
}

async function refreshDevices() {
    try {
        const devices = await invoke('get_paired_devices');
        const list = document.getElementById('pairedList');
        const calList = document.getElementById('calibrateList');
        list.innerHTML = '';
        if (calList) calList.innerHTML = '';

        if (devices.length === 0) {
            list.innerHTML = '<div class="empty-state">No paired devices. Scan to pair.</div>';
        } else {
            devices.forEach(d => {
                const div = document.createElement('div');
                div.className = 'device-card';
                div.innerHTML = `
                    <div class="device-info">
                        <h4>${d.name}</h4>
                        <p>${d.address} • RSSI baseline: ${d.baseline_rssi} dBm</p>
                    </div>
                    <div>
                        <button class="btn btn-secondary" onclick="calibrateDevice('${d.id}', this)">📡 Calibrate</button>
                        <button class="btn btn-danger" onclick="forgetDevice('${d.id}')" style="margin-left:6px;">🗑</button>
                    </div>`;
                list.appendChild(div);

                if (calList) {
                    const cdiv = document.createElement('div');
                    cdiv.style.marginBottom = '8px';
                    cdiv.dataset.calibrationDevice = d.id;
                    cdiv.innerHTML = `
                        <div style="display:flex;justify-content:space-between;align-items:center;">
                            <span style="font-size:14px;">${d.name}</span>
                            <button class="btn btn-secondary" onclick="calibrateDevice('${d.id}', this)">📡 Calibrate</button>
                        </div>
                        <progress class="calibration-progress" value="0" max="100" style="width:100%;margin-top:6px;"></progress>
                        <div class="calibration-progress-text" style="font-size:12px;opacity:.75;margin-top:3px;">Ready</div>`;
                    calList.appendChild(cdiv);
                }
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
            list.innerHTML = '<div class="empty-state">No devices found. Make sure watch app is open.</div>';
        } else {
            found.forEach(d => {
                const div = document.createElement('div');
                div.className = 'device-card';
                div.innerHTML = `
                    <div class="device-info">
                        <h4>${d.name}</h4>
                        <p>RSSI: ${d.rssi} dBm • ${d.address}</p>
                    </div>
                    <button class="btn" onclick="pairDevice('${d.id}', '${d.name.replace(/'/g, "\\'")}', ${d.rssi}, '${d.address}')">🔗 Pair</button>`;
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

async function calibrateDevice(id, button) {
    const btn = button || (typeof event !== 'undefined' ? event.target : null);
    if (btn) {
        btn.disabled = true;
        btn.textContent = '⏳ Measuring...';
    }
    try {
        const result = await invoke('calibrate_device', { id });
        alert(`📡 Calibration complete!\nMedian/average: ${result.avg} dBm\nThreshold: ${result.threshold} dBm\nSamples: ${result.samples}`);
        await refreshDevices();
    } catch (e) {
        alert('Calibration failed: ' + e);
    } finally {
        if (btn) {
            btn.disabled = false;
            btn.textContent = '📡 Calibrate';
        }
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
    } catch (e) { console.error('loadConfig failed:', e); }
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
        await invoke('update_config', { newConfig: cfg });
        alert('💾 Saved!');
    } catch (e) { alert('❌ Save failed: ' + e); }
}

async function toggleDaemon() {
    const enabled = document.getElementById('daemonToggle').checked;
    try {
        await invoke(enabled ? 'start_daemon' : 'stop_daemon');
        daemonEnabled = enabled;
        await refreshStatus();
    } catch (e) {
        alert('Toggle daemon failed: ' + e);
        document.getElementById('daemonToggle').checked = !enabled;
    }
}

async function lockNow() {
    alert('Lock command will be connected after the platform command is exposed.');
}

async function setWindowsPassword() {
    const pwd = document.getElementById('winPasswordInput').value;
    if (!pwd) { alert('Enter your Windows password'); return; }
    try {
        await invoke('set_windows_password', { password: pwd });
        alert('💾 Windows password encrypted and saved!');
        document.getElementById('winPasswordInput').value = '';
    } catch (e) { alert('❌ Failed: ' + e); }
}

async function registerCP() {
    alert('Credential Provider registration is not exposed by the current Tauri command set yet.');
}
async function unregisterCP() { alert('Credential Provider removal is not exposed by the current Tauri command set yet.'); }

async function loadLogDir() {
    try {
        const path = await invoke('get_log_dir');
        const el = document.getElementById('logDirPath');
        if (el) el.textContent = path;
    } catch (e) { console.error('loadLogDir failed:', e); }
}

function copyLogDir() {
    const el = document.getElementById('logDirPath');
    if (!el || el.textContent === 'Loading…') return;
    navigator.clipboard.writeText(el.textContent).then(() => {
        const btn = document.getElementById('copyLogDirBtn');
        const old = btn.textContent;
        btn.textContent = '✅ Copied!';
        setTimeout(() => btn.textContent = old, 1500);
    });
}

function showTab(tab) {
    document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));
    document.querySelectorAll('.tab-btn').forEach(el => el.classList.remove('active'));
    const content = document.getElementById('tab-' + tab);
    const button = document.querySelector(`[data-tab="${tab}"]`);
    if (content) content.classList.add('active');
    if (button) button.classList.add('active');
}

document.addEventListener('DOMContentLoaded', () => {
    setupCalibrationListeners();
    document.querySelectorAll('.tab-btn').forEach(btn => btn.addEventListener('click', () => showTab(btn.dataset.tab)));
    document.getElementById('daemonToggle')?.addEventListener('change', toggleDaemon);
    document.getElementById('scanBtn')?.addEventListener('click', scanDevices);
    document.getElementById('saveConfigBtn')?.addEventListener('click', saveConfig);
    document.getElementById('lockNowBtn')?.addEventListener('click', lockNow);
    document.getElementById('cpRegisterBtn')?.addEventListener('click', registerCP);
    document.getElementById('cpUnregisterBtn')?.addEventListener('click', unregisterCP);
    document.getElementById('winPasswordBtn')?.addEventListener('click', setWindowsPassword);
    document.getElementById('copyLogDirBtn')?.addEventListener('click', copyLogDir);
    refreshStatus();
    refreshDevices();
    loadConfig();
    loadLogDir();
    setInterval(refreshStatus, 3000);
});
