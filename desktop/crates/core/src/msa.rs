//! Client for the WristKey MSA server (`wristkey-msa`), used by PCs without
//! Bluetooth. The watch registers itself with the server over HTTP; the PC then
//! resolves its own binding by the MSA account it authenticates with, which is
//! how it learns the `wristkey_id` the server minted for it.

use serde::{Deserialize, Serialize};

/// One watch binding as returned by the server.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MsaBinding {
    pub wristkey_id: String,
    pub pc_name: String,
    pub msa_account: String,
    /// SEC1 uncompressed `04 || X || Y`, base64.
    pub watch_pubkey_b64: String,
    pub linked_at: String,
}

/// Response of `GET /api/v1/account/{msa_account}/bindings`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MsaAccountBindings {
    pub msa_account: String,
    pub bindings: Vec<MsaBinding>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum MsaError {
    #[error("msa server url is not configured")]
    NotConfigured,
    #[error("msa server rejected the request: {0}")]
    Unauthorized(String),
    #[error("msa request failed: {0}")]
    Request(String),
    #[error("msa server response was malformed: {0}")]
    Malformed(String),
}

/// Async HTTP client for the MSA server.
///
/// `base_url` is expected without a trailing path, e.g. `http://10.0.0.5:8787`.
#[derive(Clone, Debug)]
pub struct MsaClient {
    base_url: String,
    token: Option<String>,
}

impl MsaClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self { base_url: base_url.into().trim_end_matches('/').to_owned(), token }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let builder = self.base().request(method, self.url(path));
        match &self.token {
            Some(t) if !t.is_empty() => builder.bearer_auth(t),
            _ => builder,
        }
    }

    fn base(&self) -> reqwest::Client {
        // A fresh client per call keeps this type trivially Send/Sync without
        // holding a connection pool the caller must manage.
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    }

    /// Public endpoint; confirms the server is reachable and up.
    pub async fn health(&self) -> Result<(), MsaError> {
        let resp = self
            .request(reqwest::Method::GET, "/api/v1/health")
            .send()
            .await
            .map_err(|e| MsaError::Request(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(MsaError::Request(format!("health -> {}", resp.status())))
        }
    }

    /// Newest-first bindings registered for `msa_account`. An empty vector means
    /// the watch has not registered against this server yet.
    pub async fn bindings_for_account(&self, msa_account: &str) -> Result<Vec<MsaBinding>, MsaError> {
        let encoded = percent_encode_segment(msa_account);
        let resp = self
            .request(reqwest::Method::GET, &format!("/api/v1/account/{encoded}/bindings"))
            .send()
            .await
            .map_err(|e| MsaError::Request(e.to_string()))?;
        match resp.status() {
            s if s.is_success() => {}
            s if s == reqwest::StatusCode::UNAUTHORIZED => {
                return Err(MsaError::Unauthorized("Bearer token missing or wrong".into()))
            }
            s => return Err(MsaError::Request(format!("bindings -> {s}"))),
        }
        let parsed: MsaAccountBindings = resp
            .json()
            .await
            .map_err(|e| MsaError::Malformed(e.to_string()))?;
        Ok(parsed.bindings)
    }

    /// Resolve this PC's own binding: newest one for `msa_account`.
    pub async fn resolve_own_binding(&self, msa_account: &str) -> Result<Option<MsaBinding>, MsaError> {
        Ok(self.bindings_for_account(msa_account).await?.into_iter().next())
    }

    /// Fetch one binding by the id the server minted.
    pub async fn binding_by_id(&self, wristkey_id: &str) -> Result<Option<MsaBinding>, MsaError> {
        let encoded = percent_encode_segment(wristkey_id);
        let resp = self
            .request(reqwest::Method::GET, &format!("/api/v1/status/{encoded}"))
            .send()
            .await
            .map_err(|e| MsaError::Request(e.to_string()))?;
        match resp.status() {
            s if s.is_success() => {}
            s if s == reqwest::StatusCode::NOT_FOUND => return Ok(None),
            s if s == reqwest::StatusCode::UNAUTHORIZED => {
                return Err(MsaError::Unauthorized("Bearer token missing or wrong".into()))
            }
            s => return Err(MsaError::Request(format!("status -> {s}"))),
        }
        let binding: MsaBinding = resp
            .json()
            .await
            .map_err(|e| MsaError::Malformed(e.to_string()))?;
        Ok(Some(binding))
    }
}

/// Percent-encode a value used as a single URL path segment.
///
/// MSA accounts contain `@` and Windows account names contain backslashes and
/// spaces; an unencoded `\` would silently change the request target on the
/// server side.
pub fn percent_encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, EcdsaP256Crypto, MemoryStorage, SessionManager};
    use std::sync::Arc;

    fn session() -> SessionManager {
        SessionManager::new(Arc::new(EcdsaP256Crypto), Arc::new(MemoryStorage::new()))
    }

    fn binding() -> MsaBinding {
        MsaBinding {
            wristkey_id: "wk-12345678-1234-4234-8234-123456789abc".into(),
            pc_name: "DESK-LAN".into(),
            msa_account: "user@outlook.com".into(),
            watch_pubkey_b64: "BAtzbWFpbwo=".into(),
            linked_at: "2026-09-22T10:00:00+00:00".into(),
        }
    }

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(fut)
    }

    #[test]
    fn wristkey_id_maps_to_stable_device_uuid() {
        let a = crate::uuid_from_wristkey_id("wk-12345678-1234-4234-8234-123456789abc").unwrap();
        let b = crate::uuid_from_wristkey_id("wk-12345678-1234-4234-8234-123456789abc").unwrap();
        assert_eq!(a, b, "the LAN password slot must survive restarts");
        assert_eq!(a.to_string(), "12345678-1234-4234-8234-123456789abc");

        // Bare uuid without the prefix is accepted too.
        assert_eq!(crate::uuid_from_wristkey_id("12345678-1234-4234-8234-123456789abc").unwrap(), a);

        assert!(crate::uuid_from_wristkey_id("not-a-uuid").is_err());
        assert!(crate::uuid_from_wristkey_id("").is_err());
    }

    #[test]
    fn lan_slot_is_keyed_by_account_not_wristkey_id() {
        // The server mints a NEW wristkey_id on every re-register, so the slot
        // must follow the MSA account or the password would be lost.
        let a = binding();
        let mut reregistered = a.clone();
        reregistered.wristkey_id = "wk-99999999-9999-4999-8999-999999999999".into();

        let first = crate::lan_device_id(&a.msa_account).unwrap();
        let second = crate::lan_device_id(&reregistered.msa_account).unwrap();
        assert_eq!(first, second, "same account must map to the same slot");
        assert_ne!(a.wristkey_id, reregistered.wristkey_id, "precondition: the id really rotates");

        // Different accounts get independent slots; whitespace is normalized.
        assert_ne!(crate::lan_device_id("alice@outlook.com").unwrap(), crate::lan_device_id("bob@outlook.com").unwrap());
        assert_eq!(crate::lan_device_id("  user@outlook.com ").unwrap(), crate::lan_device_id("user@outlook.com").unwrap());
        assert!(crate::lan_device_id("   ").is_err());
    }

    #[test]
    fn password_survives_wristkey_id_rotation() {
        let s = session();
        let a = binding();
        block_on(s.set_lan_device_password(&a, b"secret".to_vec())).unwrap();

        let mut rotated = a.clone();
        rotated.wristkey_id = "wk-99999999-9999-4999-8999-999999999999".into();
        assert_eq!(
            block_on(s.get_lan_device_password(&rotated)).unwrap().unwrap(),
            b"secret".to_vec(),
            "re-registering on the watch must not lose the stored password"
        );
        // The rotated id's metadata is adopted into the same single record.
        assert_eq!(block_on(s.list_paired_devices()).unwrap().len(), 1);
    }

    #[test]
    fn lan_device_record_is_created_from_binding() {
        let s = session();
        let b = binding();
        let device = block_on(s.upsert_lan_device(&b)).unwrap();
        assert_eq!(device.name, "DESK-LAN");
        assert_eq!(device.public_key, crate::base64_decode(&b.watch_pubkey_b64));
        assert!(device.windows_password.is_none());

        let devices = block_on(s.list_paired_devices()).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, device.id);
    }

    #[test]
    fn lan_password_round_trips_and_survives_re_upsert() {
        let s = session();
        let b = binding();
        assert!(block_on(s.get_lan_device_password(&b)).unwrap().is_none());

        block_on(s.set_lan_device_password(&b, b"encrypted-secret".to_vec())).unwrap();
        assert_eq!(
            block_on(s.get_lan_device_password(&b)).unwrap().unwrap(),
            b"encrypted-secret".to_vec()
        );

        // Re-resolving the same binding (e.g. after a watch re-register round) must
        // refresh metadata WITHOUT clearing the stored password.
        let mut refreshed = b.clone();
        refreshed.pc_name = "DESK-LAN-RENAMED".into();
        let device = block_on(s.upsert_lan_device(&refreshed)).unwrap();
        assert_eq!(device.name, "DESK-LAN-RENAMED");
        assert_eq!(
            block_on(s.get_lan_device_password(&b)).unwrap().unwrap(),
            b"encrypted-secret".to_vec(),
            "upsert must never drop a stored password"
        );
    }

    #[test]
    fn different_bindings_get_independent_password_slots() {
        let s = session();
        let a = binding();
        let mut b2 = binding();
        b2.msa_account = "other@outlook.com".into();
        block_on(s.set_lan_device_password(&a, b"first".to_vec())).unwrap();
        block_on(s.set_lan_device_password(&b2, b"second".to_vec())).unwrap();
        assert_eq!(block_on(s.get_lan_device_password(&a)).unwrap().unwrap(), b"first".to_vec());
        assert_eq!(block_on(s.get_lan_device_password(&b2)).unwrap().unwrap(), b"second".to_vec());
        assert_eq!(block_on(s.list_paired_devices()).unwrap().len(), 2);
    }

    #[test]
    fn empty_watch_pubkey_does_not_wipe_existing_key() {
        let s = session();
        let b = binding();
        let expected = crate::base64_decode(&b.watch_pubkey_b64);
        block_on(s.upsert_lan_device(&b)).unwrap();
        let mut no_key = b.clone();
        no_key.watch_pubkey_b64 = String::new();
        let device = block_on(s.upsert_lan_device(&no_key)).unwrap();
        assert_eq!(device.public_key, expected, "an unparseable pubkey must not clear the key");
    }

    #[test]
    fn crypto_engine_is_usable_in_core_tests() {
        // Sanity check that the test session is wired to a working engine.
        let s = session();
        assert!(block_on(s.list_paired_devices()).unwrap().is_empty());
    }

    #[test]
    fn base_url_drops_trailing_slashes() {
        assert_eq!(MsaClient::new("http://10.0.0.5:8787/", None).base_url(), "http://10.0.0.5:8787");
        assert_eq!(MsaClient::new("http://10.0.0.5:8787///", None).base_url(), "http://10.0.0.5:8787");
        assert_eq!(MsaClient::new("http://10.0.0.5:8787", None).base_url(), "http://10.0.0.5:8787");
    }

    #[test]
    fn urls_are_built_from_base() {
        let c = MsaClient::new("http://10.0.0.5:8787", None);
        assert_eq!(c.url("/api/v1/health"), "http://10.0.0.5:8787/api/v1/health");
        assert_eq!(
            c.url(&format!("/api/v1/account/{}/bindings", percent_encode_segment("user@outlook.com"))),
            "http://10.0.0.5:8787/api/v1/account/user%40outlook.com/bindings"
        );
    }

    #[test]
    fn percent_encodes_path_reserved_characters() {
        assert_eq!(percent_encode_segment("user@outlook.com"), "user%40outlook.com");
        assert_eq!(percent_encode_segment("DOMAIN\\user"), "DOMAIN%5Cuser");
        assert_eq!(percent_encode_segment("a b"), "a%20b");
        assert_eq!(percent_encode_segment("a/b"), "a%2Fb");
        assert_eq!(percent_encode_segment("plain-token_1.0~x"), "plain-token_1.0~x");
        // '#' and '?' would truncate the path if left raw.
        assert_eq!(percent_encode_segment("a#b?c"), "a%23b%3Fc");
        assert_eq!(percent_encode_segment("%"), "%25");
        assert_eq!(percent_encode_segment(""), "");
    }

    #[test]
    fn binding_json_roundtrips() {
        let raw = r#"{
            "wristkey_id": "wk-1234",
            "pc_name": "DESK-MSA1",
            "msa_account": "user@outlook.com",
            "watch_pubkey_b64": "BAtzbWFpbwo=",
            "linked_at": "2026-09-22T10:00:00+00:00"
        }"#;
        let b: MsaBinding = serde_json::from_str(raw).unwrap();
        assert_eq!(b.wristkey_id, "wk-1234");
        assert_eq!(b.pc_name, "DESK-MSA1");
        assert_eq!(b.msa_account, "user@outlook.com");
        assert_eq!(b.watch_pubkey_b64, "BAtzbWFpbwo=");
        assert_eq!(b.linked_at, "2026-09-22T10:00:00+00:00");

        // Re-serializing must produce the same field set the server sends, so
        // the daemon can hand the value straight back as its JSON reply.
        let again: MsaBinding = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(again, b);
    }

    #[test]
    fn account_bindings_response_parses_empty_list() {
        let raw = r#"{"msa_account":"user@outlook.com","bindings":[]}"#;
        let parsed: MsaAccountBindings = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.msa_account, "user@outlook.com");
        assert!(parsed.bindings.is_empty(), "no binding yet is a normal state");
    }

    #[test]
    fn account_bindings_response_parses_multiple_newest_first() {
        let raw = r#"{"msa_account":"a@b.c","bindings":[
            {"wristkey_id":"wk-new","pc_name":"P1","msa_account":"a@b.c","watch_pubkey_b64":"x","linked_at":"2026-09-22T12:00:00+00:00"},
            {"wristkey_id":"wk-old","pc_name":"P1","msa_account":"a@b.c","watch_pubkey_b64":"y","linked_at":"2026-09-22T09:00:00+00:00"}
        ]}"#;
        let parsed: MsaAccountBindings = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.bindings.len(), 2);
        assert_eq!(parsed.bindings[0].wristkey_id, "wk-new");
    }

    #[test]
    fn config_defaults_leave_lan_disabled_and_serialize() {
        let c = Config::default();
        assert_eq!(c.msa_server_url, "", "LAN mode is opt-in");
        assert_eq!(c.msa_token, "");
        assert_eq!(c.msa_account, "");

        // Old config files without the new keys must still load.
        let legacy: Config = toml::from_str(
            "auto_lock_timeout_sec = 45\nrssi_threshold_offset_dbm = 20\nchallenge_timeout_sec = 15\n",
        )
        .unwrap();
        assert_eq!(legacy.auto_lock_timeout_sec, 45);
        assert_eq!(legacy.msa_server_url, "");

        let mut with_lan = c.clone();
        with_lan.msa_server_url = "http://10.0.0.5:8787".into();
        with_lan.msa_token = "tok".into();
        with_lan.msa_account = "user@outlook.com".into();
        let toml_text = toml::to_string_pretty(&with_lan).unwrap();
        assert!(toml_text.contains("msa_server_url = \"http://10.0.0.5:8787\""));
        let back: Config = toml::from_str(&toml_text).unwrap();
        assert_eq!(back.msa_token, "tok");
        assert_eq!(back.msa_account, "user@outlook.com");
    }
}