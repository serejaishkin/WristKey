//! wristkeyctl -- drive the WristKey service pipe from a console.
//!
//! Needs an Administrator console (the pipe is restricted to SYSTEM and
//! Administrators). `mock ...` commands only work when the service runs with the
//! environment variable WRISTKEY_MOCK_WATCH=1 (machine-level), and then the
//! service does NOT talk to a real watch for sign-in pre-authentication.
//!
//!   wristkeyctl state | watch
//!   wristkeyctl ui-shown | ui-hidden | user-locked | manual | reset | fail
//!   wristkeyctl on-body true|false|none
//!   wristkeyctl consume <grant_id>
//!   wristkeyctl mock presence on|off|auto
//!   wristkeyctl mock approve [password] | mock deny
//!   wristkeyctl raw '{"action":"session_state"}'
//!   Options: --pipe <path>   (default \\.\pipe\wristkey)

use std::io::{BufRead, BufReader, Write};

const DEFAULT_PIPE: &str = r"\\.\pipe\wristkey";

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Maps CLI words to one JSON request line. Err = usage message.
pub fn build_request(args: &[String]) -> Result<String, String> {
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let simple = |action: &str| Ok(format!("{{\"action\":\"{}\"}}", action));
    match a.as_slice() {
        ["state"] => simple("session_state"),
        ["ui-shown"] => simple("ui_shown"),
        ["ui-hidden"] => simple("ui_hidden"),
        ["user-locked"] => simple("user_locked"),
        ["manual"] => simple("manual_unlock"),
        ["reset"] => simple("reset_session"),
        ["fail"] => simple("unlock_failed"),
        ["on-body", v] => match *v {
            "true" => Ok("{\"action\":\"on_body\",\"value\":true}".into()),
            "false" => Ok("{\"action\":\"on_body\",\"value\":false}".into()),
            "none" => Ok("{\"action\":\"on_body\",\"value\":null}".into()),
            _ => Err("on-body expects true|false|none".into()),
        },
        ["consume", id] => Ok(format!("{{\"action\":\"consume\",\"grant_id\":\"{}\"}}", json_escape(id))),
        ["mock", "presence", v] => match *v {
            "on" => Ok("{\"action\":\"mock_presence\",\"value\":true}".into()),
            "off" => Ok("{\"action\":\"mock_presence\",\"value\":false}".into()),
            "auto" => Ok("{\"action\":\"mock_presence\",\"value\":null}".into()),
            _ => Err("mock presence expects on|off|auto".into()),
        },
        ["mock", "approve"] => Ok("{\"action\":\"mock_approve\",\"password\":\"mock-password\"}".into()),
        ["mock", "approve", pw] => Ok(format!("{{\"action\":\"mock_approve\",\"password\":\"{}\"}}", json_escape(pw))),
        ["mock", "deny"] => simple("mock_deny"),
        ["raw", json] => Ok((*json).to_string()),
        _ => Err("unknown command; see the header of wristkeyctl/src/main.rs".into()),
    }
}

fn round_trip(pipe: &str, request: &str) -> std::io::Result<String> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(pipe)?;
    let mut w = f.try_clone()?;
    w.write_all(request.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    let mut line = String::new();
    BufReader::new(f).read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut pipe = DEFAULT_PIPE.to_string();
    if let Some(i) = args.iter().position(|a| a == "--pipe") {
        if i + 1 < args.len() {
            pipe = args[i + 1].clone();
            args.drain(i..=i + 1);
        }
    }
    if args.first().map(|s| s.as_str()) == Some("watch") {
        let mut last = String::new();
        loop {
            match round_trip(&pipe, "{\"action\":\"session_state\"}") {
                Ok(l) if l != last => { println!("{}", l); last = l; }
                Ok(_) => {}
                Err(e) => { if last != "!" { eprintln!("pipe: {}", e); last = "!".into(); } }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    match build_request(&args) {
        Ok(req) => match round_trip(&pipe, &req) {
            Ok(resp) => println!("{}", resp),
            Err(e) => { eprintln!("cannot talk to {}: {} (service running? Administrator console?)", pipe, e); std::process::exit(1); }
        },
        Err(m) => { eprintln!("{}", m); std::process::exit(2); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(s: &str) -> Result<String, String> {
        build_request(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }
    #[test]
    fn simple_commands() {
        assert_eq!(r("state").unwrap(), r#"{"action":"session_state"}"#);
        assert_eq!(r("ui-shown").unwrap(), r#"{"action":"ui_shown"}"#);
        assert_eq!(r("fail").unwrap(), r#"{"action":"unlock_failed"}"#);
    }
    #[test]
    fn args_with_values() {
        assert_eq!(r("on-body none").unwrap(), r#"{"action":"on_body","value":null}"#);
        assert_eq!(r("mock presence on").unwrap(), r#"{"action":"mock_presence","value":true}"#);
        assert_eq!(r("consume abc").unwrap(), r#"{"action":"consume","grant_id":"abc"}"#);
        assert!(r("on-body maybe").is_err());
        assert!(r("bogus").is_err());
    }
    #[test]
    fn password_is_escaped() {
        let req = build_request(&["mock".into(), "approve".into(), "a\"b\\c".into()]).unwrap();
        assert_eq!(req, r#"{"action":"mock_approve","password":"a\"b\\c"}"#);
    }
}
