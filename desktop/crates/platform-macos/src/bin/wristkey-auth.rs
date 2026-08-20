//! macOS authentication bridge boundary.
//! This helper intentionally does not automate password typing yet.
//! It accepts a one-shot local proof only after the WristKey daemon has
//! verified the Watch's ECDSA challenge/response.

use std::{env, fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

const TTL_MS: u128 = 5_000;

fn proof_path(device_id: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".wristkey").join(format!("auth-proof-{}", sanitize(device_id)))
}

fn sanitize(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("issue-proof") => {
            let device = args.next().ok_or("device id required")?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            let path = proof_path(&device);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(&path, format!("{}\n", now))?;
            println!("proof-issued");
        }
        Some("consume-proof") => {
            let device = args.next().ok_or("device id required")?;
            let path = proof_path(&device);
            let text = fs::read_to_string(&path)?;
            let timestamp: u128 = text.lines().next().unwrap_or("0").parse()?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            if now.saturating_sub(timestamp) > TTL_MS { fs::remove_file(&path).ok(); return Err("proof expired".into()); }
            fs::remove_file(&path)?;
            println!("proof-valid");
        }
        _ => {
            eprintln!("usage: wristkey-auth issue-proof <device-id> | consume-proof <device-id>");
            std::process::exit(2);
        }
    }
    Ok(())
}
