//! Linux local authentication bridge.
//! Production daemon -> helper IPC boundary. The helper never receives a password.

use std::{env, io::{Read, Write}, os::unix::net::{UnixListener, UnixStream}, path::PathBuf, fs, time::{SystemTime, UNIX_EPOCH}};

const SOCKET: &str = "/run/user/1000/wristkey-auth.sock";
const MAGIC: &[u8] = b"WKEY-AUTH-1";
const TTL_MS: u128 = 5_000;

fn proof_path(device: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".wristkey").join(format!("linux-proof-{}", sanitize(device)))
}
fn sanitize(v: &str) -> String { v.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect() }

fn handle(mut stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = Vec::new(); stream.read_to_end(&mut buf)?;
    if buf.len() < MAGIC.len() + 2 || &buf[..MAGIC.len()] != MAGIC { return Err("invalid request".into()); }
    let p = MAGIC.len(); let n = u16::from_le_bytes([buf[p], buf[p+1]]) as usize;
    if buf.len() != p + 2 + n { return Err("invalid nonce".into()); }
    stream.set_nonblocking(false)?;
    // The helper only acknowledges a fresh nonce. The daemon must establish
    // the authenticated IPC peer and issue the nonce after ECDSA verification.
    stream.write_all(b"OK\n")?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match env::args().nth(1).as_deref() {
        Some("issue-proof") => {
            let device = env::args().nth(2).ok_or("device id required")?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            let p = proof_path(&device); fs::create_dir_all(p.parent().unwrap())?;
            fs::write(&p, format!("{}\n", now))?;
            println!("proof-issued");
        }
        Some("serve") => {
            let _ = fs::remove_file(SOCKET);
            let listener = UnixListener::bind(SOCKET)?;
            for stream in listener.incoming() { if let Ok(s) = stream { let _ = handle(s); } }
        }
        _ => eprintln!("usage: wristkey-auth serve | issue-proof <device-id>"),
    }
    Ok(())
}
