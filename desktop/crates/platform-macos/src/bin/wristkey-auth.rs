//! macOS authentication helper boundary.
//! The helper is separate from Tauri/BLE and owns the final credential boundary.
//! It deliberately does not synthesize keyboard input.

use std::{env, fs, io::{BufRead, BufReader, Write}, os::unix::fs::PermissionsExt, os::unix::net::{UnixListener, UnixStream}, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

const TTL_MS: u128 = 5_000;

fn socket_path() -> PathBuf {
    env::var("WRISTKEY_AUTH_SOCKET").map(PathBuf::from).unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".wristkey").join("auth.sock")
    })
}
fn proof_path(device_id: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".wristkey").join(format!("auth-proof-{}", sanitize(device_id)))
}
fn sanitize(value: &str) -> String { value.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect() }
fn now_ms() -> Result<u128, Box<dyn std::error::Error>> { Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()) }

fn issue_proof(device: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = proof_path(device); fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, format!("{}\n", now_ms()?))?;
    let mut perms = fs::metadata(&path)?.permissions(); perms.set_mode(0o600); fs::set_permissions(&path, perms)?; Ok(())
}
fn consume_proof(device: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = proof_path(device); let text = fs::read_to_string(&path)?;
    let timestamp: u128 = text.lines().next().unwrap_or("0").parse()?;
    if now_ms()?.saturating_sub(timestamp) > TTL_MS { let _ = fs::remove_file(&path); return Err("proof expired".into()); }
    fs::remove_file(&path)?; Ok(())
}
fn handle(mut stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut line = String::new(); BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("authorize") => {
            let device = parts.next().ok_or("device id required")?;
            consume_proof(device)?;
            // Password stays inside the Keychain/helper boundary. The native
            // authentication adapter is intentionally separate from this IPC layer.
            stream.write_all(b"authorized\n")?;
        }
        _ => stream.write_all(b"error invalid-command\n")?,
    }
    Ok(())
}
fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let path = socket_path(); if path.exists() { fs::remove_file(&path)?; }
    fs::create_dir_all(path.parent().unwrap())?;
    let listener = UnixListener::bind(&path)?;
    let mut perms = fs::metadata(&path)?.permissions(); perms.set_mode(0o600); fs::set_permissions(&path, perms)?;
    for stream in listener.incoming() { match stream { Ok(stream) => { let _ = handle(stream); }, Err(_) => break } }
    let _ = fs::remove_file(path); Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("serve") => serve(),
        Some("issue-proof") => { issue_proof(&args.next().ok_or("device id required")?)?; println!("proof-issued"); Ok(()) }
        Some("consume-proof") => { consume_proof(&args.next().ok_or("device id required")?)?; println!("proof-valid"); Ok(()) }
        Some("authorize") => {
            let device = args.next().ok_or("device id required")?; let mut stream = UnixStream::connect(socket_path())?;
            writeln!(stream, "authorize {}", sanitize(&device))?; let mut response = String::new(); BufReader::new(stream).read_line(&mut response)?; print!("{}", response); Ok(())
        }
        _ => { eprintln!("usage: wristkey-auth serve | issue-proof <device-id> | consume-proof <device-id> | authorize <device-id>"); std::process::exit(2); }
    }
}
