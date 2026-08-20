/* WristKey Desktop Frontend */

let currentState = 'disconnected';
let currentDeviceCount = 0;
let daemonEnabled = false;
let cpRegistered = false;

async function invoke(cmd, args = {}) {
    try { return await window.__TAURI__.core.invoke(cmd, args); }
    catch (e) { console.error(`Invoke ${cmd} failed:`, e); throw e; }
}

async function refreshStatus() {
    try {
        const status = await invoke('get_status');
        currentState = status.state; currentDeviceCount = status.device_count;
        daemonEnabled = status.daemon_enabled; cpRegistered = status.cp_registered || false;
        document.getElementById('statusState').textContent = status.detail;
        document.getElementById('statusDot').className = 'status-dot ' + status.state;
        document.getElementById('deviceCount').textContent = status.device_count;
        document.getElementById('daemonToggle').checked = status.daemon_enabled;
        const cpTabBtn = document.getElementById('tab-btn-cp'); if (cpTabBtn) cpTabBtn.classList.remove('hidden');
        const cpStatus = document.getElementById('cpStatus');
        if (cpStatus) { cpStatus.textContent = cpRegistered ? '✅ Registered' : '❌ Not registered'; cpStatus.className = cpRegistered ? 'status-ok' : 'status-warn'; }
        const cpBtn = document.getElementById('cpRegisterBtn');
        if (cpBtn) cpBtn.textContent = cpRegistered ? '🔁 Re-register CP' : '📝 Register Credential Provider';
        const storageType = document.getElementById('storageType');
        if (storageType && status.storage_type) {
            storageType.textContent = status.storage_type;
            if (status.storage_type.includes('TPM')) storageType.innerHTML = '🔒 TPM 2.0';
            else if (status.storage_type.includes('Software')) storageType.innerHTML = '💻 Software';
        }
    } catch (e) { document.getElementById('statusState').textContent = 'Error: ' + e; }
}

async function refreshDevices() {
    try {
        const devices = await invoke('get_paired_devices');
        const list = document.getElementById('pairedList'); const calList = document.getElementById('calibrateList');
        list.innerHTML = ''; if (calList) calList.innerHTML = '';
        if (devices.length === 0) {
            list.innerHTML = '<div class="empty-state">No paired devices. Scan to pair.</div>';
        } else {
            devices.forEach(d => {
                const div = document.createElement('div'); div.className = 'device-card';
                div.innerHTML = `<div class="device-info"><h4>${d.name}</h4><p>${d.address} • RSSI baseline: ${d.baseline_rssi} dBm</p></div><div><button class="btn btn-secondary" onclick="calibrateDevice('${d.id}')">📡 Calibrate</button><button class="btn btn-danger" onclick="forgetDevice('${d.id}')" style="margin-left:6px;">🗑</button></div>`;
                list.appendChild(div);
                if (calList) {
                    const cdiv = document.createElement('div'); cdiv.style.marginBottom = '8px';
                    cdiv.innerHTML = `<div style="display:flex;justify-content:space-between;align-items:center;"><span style="font-size:14px;">${d.name}</span><button class="btn btn-secondary" onclick="calibrateDevice('${d.id}')">📡 Calibrate</button></div>`;
                    calList.appendChild(cdiv);
                }
            });
        }
    } catch (e) { console.error('refreshDevices failed:', e); }
}

async function scanDevices() {
    const btn = document.getElementById('scanBtn'); btn.disabled = true; btn.textContent = '🔍 Scanning...';
    try {
        const found = await invoke('scan_devices'); const list = document.getElementById('scanList'); list.innerHTML = '';
        const uniqueDevices = new Map();
        found.forEach(d => { if (!uniqueDevices.has(d.address) || (d.rssi > uniqueDevices.get(d.address).rssi)) uniqueDevices.set(d.address, d); });
        const deduplicated = Array.from(uniqueDevices.values());
        if (deduplicated.length === 0) list.innerHTML = '<div class="empty-state">No WristKey devices found. Make sure watch app is open and advertising.</div>';
        else deduplicated.forEach(d => {
            const div = document.createElement('div'); div.className = 'device-card';
            const pinDisplay = d.pin ? `<span style="color:#00ff00;font-weight:bold;font-size:16px;">PIN: ${d.pin}</span>` : '';
            div.innerHTML = `<div class="device-info"><h4>${d.name}</h4><p>${pinDisplay} RSSI: ${d.rssi} dBm • ${d.address}</p></div><button class="btn" onclick="pairDevice('${d.id}', '${d.name.replace(/'/g, "\\'")}', ${d.rssi}, '${d.address}', this)">🔗 Pair</button>`;
            list.appendChild(div);
        });
    } catch (e) { alert('Scan failed: ' + e); }
    finally { btn.disabled = false; btn.textContent = '🔍 Scan for New Device'; }
}

async function measureRssi() {
    const btn = document.getElementById('diagRefreshBtn');
    if (!btn) return;
    btn.disabled = true; btn.textContent = '⏳ Measuring...';
    const stateEl = document.getElementById('diagState');
    try {
        const devices = await invoke('get_paired_devices');
        if (!devices.length) throw new Error('No paired Watch');
        const paired = devices[0];
        const found = await invoke('scan_devices');
        const candidates = found.filter(d => d.address === paired.address || d.id === paired.address);
        if (!candidates.length) {
            document.getElementById('diagWatch').textContent = paired.name;
            document.getElementById('diagAddress').textContent = paired.address;
            document.getElementById('diagRssi').textContent = '—';
            document.getElementById('diagBaseline').textContent = `${paired.baseline_rssi}`;
            document.getElementById('diagDelta').textContent = '—';
            stateEl.textContent = 'AWAY / not visible in scan'; stateEl.className = 'status-warn';
            return;
        }
        const sample = candidates.reduce((best, d) => d.rssi > best.rssi ? d : best, candidates[0]);
        const delta = sample.rssi - paired.baseline_rssi;
        document.getElementById('diagWatch').textContent = paired.name;
        document.getElementById('diagAddress').textContent = paired.address;
        document.getElementById('diagRssi').textContent = `${sample.rssi}`;
        document.getElementById('diagBaseline').textContent = `${paired.baseline_rssi}`;
        document.getElementById('diagDelta').textContent = `${delta > 0 ? '+' : ''}${delta}`;
        if (delta >= -5) { stateEl.textContent = 'PRESENT / strong'; stateEl.className = 'status-ok'; }
        else if (delta >= -15) { stateEl.textContent = 'NEAR / usable'; stateEl.className = 'status-ok'; }
        else { stateEl.textContent = 'WEAK / candidate away'; stateEl.className = 'status-warn'; }
    } catch (e) {
        stateEl.textContent = 'Measurement failed: ' + e; stateEl.className = 'status-warn';
    } finally { btn.disabled = false; btn.textContent = '📡 Measure RSSI'; }
}

async function pairDevice(id, name, rssi, address, btnElement) {
    const btn = btnElement || event.target;
    try {
        btn.disabled = true; btn.textContent = '⏳ Waiting for watch confirmation...';
        await invoke('pair_device', { req: { id, name, rssi, address } });
        alert('✅ Paired successfully!'); await refreshDevices(); await refreshStatus();
    } catch (e) { alert('❌ Pairing failed: ' + e); }
    finally { btn.disabled = false; btn.textContent = '🔗 Pair'; }
}

async function forgetDevice(id) {
    if (!confirm('Forget this device?')) return;
    try { await invoke('forget_device', { id }); await refreshDevices(); await refreshStatus(); }
    catch (e) { alert('Forget failed: ' + e); }
}

async function calibrateDevice(id) {
    const btn = event.target; btn.disabled = true; btn.textContent = '⏳ Calibrating...';
    try {
        const result = await invoke('calibrate_proximity', { id });
        alert(`📡 Calibration complete!\nMedian: ${result.avg} dBm\nThreshold: ${result.threshold} dBm\nSamples: ${result.samples}`);
    } catch (e) { alert('Calibration failed: ' + e); }
    finally { btn.disabled = false; btn.textContent = '📡 Calibrate'; }
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
        const cfg = { auto_lock_timeout_sec: parseInt(document.getElementById('timeoutInput').value), rssi_threshold_offset_dbm: parseInt(document.getElementById('rssiInput').value), challenge_timeout_sec: parseInt(document.getElementById('challengeInput').value), log_to_file: document.getElementById('logFileToggle').checked, log_to_console: document.getElementById('logConsoleToggle').checked, log_level: document.getElementById('logLevelSelect').value };
        await invoke('set_config', { config: cfg }); alert('💾 Saved!');
    } catch (e) { alert('❌ Save failed: ' + e); }
}

async function toggleDaemon() {
    const enabled = document.getElementById('daemonToggle').checked;
    try { await invoke('toggle_daemon', { enabled }); daemonEnabled = enabled; }
    catch (e) { alert('Toggle daemon failed: ' + e); document.getElementById('daemonToggle').checked = !enabled; }
}
async function lockNow() { try { await invoke('lock_screen'); } catch (e) { alert('Lock failed: ' + e); } }
async function setWindowsPassword() {
    const pwd = document.getElementById('winPasswordInput').value;
    if (!pwd) { alert('Enter your Windows password'); return; }
    try { await invoke('set_windows_password', { password: pwd }); alert('💾 Windows password encrypted and saved!'); document.getElementById('winPasswordInput').value = ''; }
    catch (e) { alert('❌ Failed: ' + e); }
}
async function registerCP() { try { await invoke('register_credential_provider'); alert('✅ Credential Provider registered! Restart PC to apply.'); await refreshStatus(); } catch (e) { alert('❌ Registration failed: ' + e); } }
async function unregisterCP() { if (!confirm('Unregister Credential Provider?')) return; try { await invoke('unregister_credential_provider'); alert('✅ Unregistered. Restart PC to apply.'); await refreshStatus(); } catch (e) { alert('❌ Failed: ' + e); } }
async function loadLogDir() { try { const path = await invoke('get_log_dir'); const el = document.getElementById('logDirPath'); if (el) el.textContent = path; } catch (e) { console.error('loadLogDir failed:', e); } }
function copyLogDir() { const el = document.getElementById('logDirPath'); if (!el || el.textContent === 'Loading…') return; navigator.clipboard.writeText(el.textContent).then(() => { const btn = document.getElementById('copyLogDirBtn'); const old = btn.textContent; btn.textContent = '✅ Copied!'; setTimeout(() => btn.textContent = old, 1500); }); }
function showTab(tab) { document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active')); document.querySelectorAll('.tab-btn').forEach(el => el.classList.remove('active')); document.getElementById('tab-' + tab).classList.add('active'); document.querySelector(`[data-tab="${tab}"]`).classList.add('active'); }

document.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('.tab-btn').forEach(btn => btn.addEventListener('click', () => showTab(btn.dataset.tab)));
    document.getElementById('daemonToggle').addEventListener('change', toggleDaemon);
    document.getElementById('scanBtn').addEventListener('click', scanDevices);
    document.getElementById('diagRefreshBtn').addEventListener('click', measureRssi);
    document.getElementById('saveConfigBtn').addEventListener('click', saveConfig);
    document.getElementById('lockNowBtn').addEventListener('click', lockNow);
    const cpBtn = document.getElementById('cpRegisterBtn'); if (cpBtn) cpBtn.addEventListener('click', registerCP);
    const cpUnregBtn = document.getElementById('cpUnregisterBtn'); if (cpUnregBtn) cpUnregBtn.addEventListener('click', unregisterCP);
    const winPwdBtn = document.getElementById('winPasswordBtn'); if (winPwdBtn) winPwdBtn.addEventListener('click', setWindowsPassword);
    const copyLogBtn = document.getElementById('copyLogDirBtn'); if (copyLogBtn) copyLogBtn.addEventListener('click', copyLogDir);
    refreshStatus(); refreshDevices(); loadConfig(); loadLogDir(); setInterval(refreshStatus, 3000);
});
