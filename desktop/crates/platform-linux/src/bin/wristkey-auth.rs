//! Linux WristKey authentication helper.
//! Local IPC boundary. The helper never receives a password.

use std::{env, fs, io::{Read, Write}, os::unix::{net::{UnixListener, UnixStream}, fs::{PermissionsExt, MetadataExt}}, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

const MAGIC: &[u8] = b"WKEY-AUTH-2";
const SOCKET_NAME: &str = "wristkey-auth.sock";
const TTL_MS: u128 = 5_000;

fn runtime_socket() -> PathBuf {
    if let Ok(dir) = env::var("XDG_RUNTIME_DIR") { return PathBuf::from(dir).join(SOCKET_NAME); }
    PathBuf::from(format!("/run/user/{}/{}", unsafe { libc::getuid() }, SOCKET_NAME))
}

fn check_socket_owner(path: &PathBuf) -> std::io::Result<()> {
    let meta = fs::metadata(path)?;
    if meta.uid() != unsafe { libc::getuid() } { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "socket owner mismatch")); }
    Ok(())
}

fn serve_client(mut stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut req = Vec::new(); stream.read_to_end(&mut req)?;
    if req.len() < MAGIC.len() + 26 || &req[..MAGIC.len()] != MAGIC { return Err("invalid auth request".into()); }
    let p = MAGIC.len();
    let mut tsb = [0u8; 8]; tsb.copy_from_slice(&req[p..p+8]);
    let timestamp = u64::from_le_bytes(tsb) as u128;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    if now.saturating_sub(timestamp) > TTL_MS { return Err("request expired".into()); }
    let n = u16::from_le_bytes([req[p+8], req[p+9]]) as usize;
    if n < 16 || req.len() != p + 10 + n { return Err("invalid nonce".into()); }
    let nonce = &req[p+10..];
    // TODO: enforce SO_PEERCRED against the expected WristKey daemon UID.
    // The daemon must only issue this request after ECDSA verification.
    let mut reply = b"WKEY-OK-2".to_vec(); reply.extend_from_slice(nonce);
    stream.write_all(&reply)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() != Some("serve") { eprintln!("usage: wristkey-auth serve"); std::process::exit(2); }
    let socket = runtime_socket();
    let _ = fs::remove_file(&socket);
    fs::create_dir_all(socket.parent().unwrap())?;
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    check_socket_owner(&socket)?;
    for incoming in listener.incoming() { if let Ok(stream) = incoming { let _ = serve_client(stream); } }
    Ok(())
}
