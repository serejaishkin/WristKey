//! Linux WristKey authentication helper.
//! Local IPC boundary. The helper never receives a password.

// This binary uses Linux/Unix-only IPC and credential APIs. Keep the real
// implementation behind a target cfg so the workspace can be checked from
// Windows without compiling Linux-only std/libc APIs.
#[cfg(target_os = "linux")]
mod linux_auth {
    use std::{env, fs, io::{Read, Write}, os::unix::{net::{UnixListener, UnixStream}, fs::{PermissionsExt, MetadataExt}}, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

    const MAGIC: &[u8] = b"WKEY-AUTH-3";
    const SOCKET_NAME: &str = "wristkey-auth.sock";
    const TTL_MS: u128 = 5_000;

    fn runtime_socket() -> PathBuf {
        if let Ok(dir) = env::var("XDG_RUNTIME_DIR") { return PathBuf::from(dir).join(SOCKET_NAME); }
        PathBuf::from(format!("/run/user/{}/{}", unsafe { libc::getuid() }, SOCKET_NAME))
    }

    fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
        let fd = std::os::fd::AsRawFd::as_raw_fd(stream);
        unsafe {
            let mut cred: libc::ucred = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            if libc::getsockopt(fd, libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) != 0 { return Err(std::io::Error::last_os_error()); }
            Ok(cred.uid)
        }
    }

    fn serve_client(mut stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
        let expected_uid = unsafe { libc::getuid() } as u32;
        if peer_uid(&stream)? != expected_uid { return Err("peer uid rejected".into()); }
        let mut req = Vec::new(); stream.read_to_end(&mut req)?;
        if req.len() < MAGIC.len() + 26 || &req[..MAGIC.len()] != MAGIC { return Err("invalid auth request".into()); }
        let p = MAGIC.len();
        let mut tsb = [0u8; 8]; tsb.copy_from_slice(&req[p..p+8]);
        let timestamp = u64::from_le_bytes(tsb) as u128;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        if now < timestamp || now - timestamp > TTL_MS { return Err("request expired".into()); }
        let n = u16::from_le_bytes([req[p+8], req[p+9]]) as usize;
        if n < 16 || req.len() != p + 10 + n { return Err("invalid nonce".into()); }
        let nonce = &req[p+10..];
        let mut reply = b"WKEY-OK-3".to_vec(); reply.extend_from_slice(nonce);
        stream.write_all(&reply)?;
        Ok(())
    }

    fn check_socket_owner(path: &PathBuf) -> std::io::Result<()> {
        let meta = fs::metadata(path)?;
        if meta.uid() != unsafe { libc::getuid() } { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "socket owner mismatch")); }
        Ok(())
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
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
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    linux_auth::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("wristkey-auth is only available on Linux");
}
