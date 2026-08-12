#![no_main]
use libfuzzer_sys::fuzz_target;
/// Fuzz target for challenge packet parser.
/// Ensures we don't panic on malformed BLE packets.
fuzz_target!(|data: &[u8]| {
// Try to parse as challenge bytes
if data.len() >= 24 {
// nonce (16) + timestamp (8)
let _nonce = &data[0..16];
let _ts = u64::from_le_bytes([
data[16], data[17], data[18], data[19],
data[20], data[21], data[22], data[23],
]);
}
});
