//! Pure state machine for the watch-assisted sign-in session (see
//! docs/AUTOLOGIN_DESIGN.md). No IO, no clocks: the caller passes `now`
//! (seconds) and executes the returned `Action`s, so every rule is unit-tested
//! on any OS.

use serde::{Deserialize, Serialize};

pub const GRANT_TTL_SECS: u64 = 15;
/// After the user locks the PC explicitly, tap-less unlock is blocked this long.
pub const USER_LOCK_BLOCK_SECS: u64 = 10;
const BACKOFF_SECS: [u64; 2] = [30, 120];
/// Failed automatic attempts before auto-unlock is disabled until a password sign-in.
pub const MAX_FAILURES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlockMode {
    /// Presence + watch on wrist, no tap (the watch enforces on-wrist + motion, else it asks for a tap).
    Silent,
    /// Presence, then the user taps on the watch.
    Confirm,
    /// Only the tile's Unlock button starts anything.
    Manual,
}

impl Default for UnlockMode {
    fn default() -> Self { UnlockMode::Confirm }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Unlocked,
    LockedUiShown,
    Preauth { needs_tap: bool },
    GrantReady { grant_id: String, expires: u64 },
    Failed { until: u64 },
    /// Too many failures: only a successful password sign-in re-enables.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    UiShown,
    UiHidden,
    /// User pressed Win+L (explicit lock).
    UserLocked,
    Presence(bool),
    /// Watch reports whether it is on the wrist (None = unsupported).
    OnBody(Option<bool>),
    /// Tile "Unlock" button.
    ManualUnlock,
    WatchApproved { grant_id: String },
    WatchDenied,
    Consume { grant_id: String },
    UnlockFailed,
    /// Signed in with a password (or anything else that proves the user is in).
    PasswordLogin,
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run challenge/response with the watch now.
    StartPreauth { needs_tap: bool },
    CancelPreauth,
    /// Tell the provider to re-query credentials (grant is ready).
    NotifyGrantReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumeError {
    NoGrant,
    WrongGrant,
    Expired,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub state: State,
    mode: UnlockMode,
    present: bool,
    on_body: Option<bool>,
    failures: u32,
    silent_block_until: u64,
    ui_shown: bool,
}

impl Session {
    pub fn new(mode: UnlockMode) -> Self {
        Self { state: State::Unlocked, mode, present: false, on_body: None,
               failures: 0, silent_block_until: 0, ui_shown: false }
    }

    pub fn set_mode(&mut self, mode: UnlockMode) { self.mode = mode; }
    pub fn failures(&self) -> u32 { self.failures }
    pub fn mode(&self) -> UnlockMode { self.mode }
    pub fn present(&self) -> bool { self.present }

    /// Silent skips the forced tap; the watch itself still demands one unless it
    /// is on the wrist (off-body sensor) with recent motion. An explicit
    /// off-wrist report or the Win+L window forces the tap from the PC side too.
    fn needs_tap(&self, now: u64) -> bool {
        !(self.mode == UnlockMode::Silent && self.on_body != Some(false) && now >= self.silent_block_until)
    }

    fn auto_start(&mut self, now: u64) -> Vec<Action> {
        if self.mode == UnlockMode::Manual || !self.present { return vec![]; }
        let needs_tap = self.needs_tap(now);
        self.state = State::Preauth { needs_tap };
        vec![Action::StartPreauth { needs_tap }]
    }

    fn back_to_locked(&mut self) { self.state = State::LockedUiShown; }

    pub fn handle(&mut self, ev: Event, now: u64) -> Vec<Action> {
        match ev {
            Event::OnBody(v) => { self.on_body = v; vec![] }
            Event::UserLocked => { self.silent_block_until = now + USER_LOCK_BLOCK_SECS; vec![] }
            Event::PasswordLogin => {
                self.failures = 0;
                self.state = State::Unlocked;
                self.ui_shown = false;
                vec![]
            }
            Event::UiHidden => {
                self.ui_shown = false;
                let cancel = matches!(self.state, State::Preauth { .. });
                if self.state != State::Disabled { self.state = State::Unlocked; }
                if cancel { vec![Action::CancelPreauth] } else { vec![] }
            }
            Event::UiShown => {
                self.ui_shown = true;
                match self.state {
                    State::Disabled | State::Failed { .. } => vec![],
                    _ => { self.back_to_locked(); self.auto_start(now) }
                }
            }
            Event::Presence(p) => {
                let was = self.present;
                self.present = p;
                match (&self.state, p) {
                    (State::LockedUiShown, true) if !was => self.auto_start(now),
                    (State::Preauth { .. }, false) => { self.back_to_locked(); vec![Action::CancelPreauth] }
                    (State::GrantReady { .. }, false) => { self.back_to_locked(); vec![] }
                    _ => vec![],
                }
            }
            Event::ManualUnlock => match self.state {
                State::LockedUiShown => {
                    self.state = State::Preauth { needs_tap: true };
                    vec![Action::StartPreauth { needs_tap: true }]
                }
                _ => vec![],
            },
            Event::WatchApproved { grant_id } => match self.state {
                State::Preauth { .. } => {
                    self.state = State::GrantReady { grant_id, expires: now + GRANT_TTL_SECS };
                    vec![Action::NotifyGrantReady]
                }
                _ => vec![],
            },
            Event::WatchDenied => {
                if matches!(self.state, State::Preauth { .. }) { self.back_to_locked(); }
                vec![]
            }
            Event::Consume { .. } => vec![], // use `consume()`; it returns a Result
            Event::UnlockFailed => {
                self.failures += 1;
                self.state = if self.failures >= MAX_FAILURES {
                    State::Disabled
                } else {
                    State::Failed { until: now + BACKOFF_SECS[(self.failures as usize - 1).min(1)] }
                };
                vec![Action::CancelPreauth]
            }
            Event::Tick => {
                match self.state.clone() {
                    State::GrantReady { expires, .. } if now >= expires => self.back_to_locked(),
                    State::Failed { until } if now >= until => {
                        if self.ui_shown { self.back_to_locked(); return self.auto_start(now); }
                        self.state = State::Unlocked;
                    }
                    _ => {}
                }
                vec![]
            }
        }
    }

    /// Single-use: a grant is removed the moment it is consumed.
    pub fn consume(&mut self, grant_id: &str, now: u64) -> Result<(), ConsumeError> {
        match self.state.clone() {
            State::GrantReady { grant_id: id, expires } => {
                if now >= expires { self.back_to_locked(); return Err(ConsumeError::Expired); }
                if id != grant_id { return Err(ConsumeError::WrongGrant); }
                self.state = State::Unlocked;
                self.ui_shown = false;
                self.failures = 0;
                Ok(())
            }
            _ => Err(ConsumeError::NoGrant),
        }
    }
}

// ---------------------------------------------------------------------------
// Hub: Session + action queue + the one-time secret held by a grant.
// ---------------------------------------------------------------------------

fn wipe(s: String) {
    let mut b = s.into_bytes();
    for byte in b.iter_mut() {
        // volatile so the compiler cannot drop the wipe as a dead store
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

/// Snapshot for `session_state` replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub state: &'static str,
    pub grant_id: Option<String>,
    pub mode: UnlockMode,
    pub present: bool,
    pub failures: u32,
    /// Seconds until the grant / backoff ends (0 if not applicable).
    pub remaining_secs: u64,
}

/// Thread-safe-by-ownership (wrap in a `Mutex`). Events may come from any task;
/// a single driver drains `take_actions()` and performs the IO.
pub struct Hub {
    session: Session,
    queue: Vec<Action>,
    secret: Option<String>,
    epoch: std::time::Instant,
}

impl Snapshot {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "state": self.state,
            "grant_id": self.grant_id,
            "mode": self.mode,
            "present": self.present,
            "failures": self.failures,
            "remaining_secs": self.remaining_secs,
        })
    }
}

impl Hub {
    pub fn new(mode: UnlockMode) -> Self {
        Self { session: Session::new(mode), queue: Vec::new(), secret: None, epoch: std::time::Instant::now() }
    }

    fn now(&self) -> u64 { self.epoch.elapsed().as_secs() }

    fn drop_secret_unless_granted(&mut self) {
        if !matches!(self.session.state, State::GrantReady { .. }) {
            if let Some(s) = self.secret.take() { wipe(s); }
        }
    }

    pub fn set_mode(&mut self, mode: UnlockMode) { self.session.set_mode(mode); }

    pub fn handle(&mut self, ev: Event) {
        let now = self.now();
        let actions = self.session.handle(ev, now);
        self.queue.extend(actions);
        self.drop_secret_unless_granted();
    }

    pub fn take_actions(&mut self) -> Vec<Action> { std::mem::take(&mut self.queue) }

    /// The watch signed: store the secret and open a grant. Returns the grant id,
    /// or None if no pre-authentication is in progress (the secret is wiped).
    pub fn approve(&mut self, secret: String) -> Option<String> {
        if !matches!(self.session.state, State::Preauth { .. }) { wipe(secret); return None; }
        let grant_id = uuid::Uuid::new_v4().to_string();
        self.secret = Some(secret);
        self.handle(Event::WatchApproved { grant_id: grant_id.clone() });
        Some(grant_id)
    }

    /// Single-use: returns the secret and removes it.
    pub fn consume(&mut self, grant_id: &str) -> Result<String, ConsumeError> {
        let now = self.now();
        let r = self.session.consume(grant_id, now);
        match r {
            Ok(()) => Ok(self.secret.take().unwrap_or_default()),
            Err(e) => { self.drop_secret_unless_granted(); Err(e) }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = self.now();
        let (state, grant_id, remaining) = match &self.session.state {
            State::Unlocked => ("unlocked", None, 0),
            State::LockedUiShown => ("locked", None, 0),
            State::Preauth { needs_tap: true } => ("preauth_tap", None, 0),
            State::Preauth { needs_tap: false } => ("preauth", None, 0),
            State::GrantReady { grant_id, expires } => ("grant_ready", Some(grant_id.clone()), expires.saturating_sub(now)),
            State::Failed { until } => ("failed", None, until.saturating_sub(now)),
            State::Disabled => ("disabled", None, 0),
        };
        Snapshot { state, grant_id, mode: self.session.mode(), present: self.session.present(),
                   failures: self.session.failures(), remaining_secs: remaining }
    }
}

impl Drop for Hub {
    fn drop(&mut self) { if let Some(s) = self.secret.take() { wipe(s); } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked_present(mode: UnlockMode) -> Session {
        let mut s = Session::new(mode);
        s.handle(Event::Presence(true), 0);
        s
    }

    #[test]
    fn confirm_flow_issues_single_use_grant() {
        let mut s = locked_present(UnlockMode::Confirm);
        assert_eq!(s.handle(Event::UiShown, 1), vec![Action::StartPreauth { needs_tap: true }]);
        assert_eq!(s.handle(Event::WatchApproved { grant_id: "g1".into() }, 3), vec![Action::NotifyGrantReady]);
        assert_eq!(s.consume("g1", 4), Ok(()));
        assert_eq!(s.consume("g1", 4), Err(ConsumeError::NoGrant));
        assert_eq!(s.state, State::Unlocked);
    }

    #[test]
    fn grant_expires_and_wrong_id_rejected() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        s.handle(Event::WatchApproved { grant_id: "g".into() }, 0);
        assert_eq!(s.consume("x", 1), Err(ConsumeError::WrongGrant));
        assert_eq!(s.consume("g", GRANT_TTL_SECS), Err(ConsumeError::Expired));
        assert_eq!(s.state, State::LockedUiShown);
    }

    #[test]
    fn tick_expires_grant() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        s.handle(Event::WatchApproved { grant_id: "g".into() }, 0);
        s.handle(Event::Tick, GRANT_TTL_SECS);
        assert_eq!(s.state, State::LockedUiShown);
    }

    #[test]
    fn manual_mode_never_auto_starts() {
        let mut s = locked_present(UnlockMode::Manual);
        assert!(s.handle(Event::UiShown, 0).is_empty());
        assert_eq!(s.handle(Event::ManualUnlock, 1), vec![Action::StartPreauth { needs_tap: true }]);
    }

    #[test]
    fn silent_skips_forced_tap_unless_off_wrist() {
        let mut s = locked_present(UnlockMode::Silent);
        assert_eq!(s.handle(Event::UiShown, 0), vec![Action::StartPreauth { needs_tap: false }]);
        let mut s = locked_present(UnlockMode::Silent);
        s.handle(Event::OnBody(Some(true)), 0);
        assert_eq!(s.handle(Event::UiShown, 0), vec![Action::StartPreauth { needs_tap: false }]);
        let mut s = locked_present(UnlockMode::Silent);
        s.handle(Event::OnBody(Some(false)), 0);
        assert_eq!(s.handle(Event::UiShown, 0), vec![Action::StartPreauth { needs_tap: true }]);
    }

    #[test]
    fn win_l_blocks_silent_briefly() {
        let mut s = locked_present(UnlockMode::Silent);
        s.handle(Event::OnBody(Some(true)), 0);
        s.handle(Event::UserLocked, 100);
        assert_eq!(s.handle(Event::UiShown, 101), vec![Action::StartPreauth { needs_tap: true }]);
        let mut s = locked_present(UnlockMode::Silent);
        s.handle(Event::OnBody(Some(true)), 0);
        s.handle(Event::UserLocked, 100);
        assert_eq!(s.handle(Event::UiShown, 100 + USER_LOCK_BLOCK_SECS),
                   vec![Action::StartPreauth { needs_tap: false }]);
    }

    #[test]
    fn walking_up_starts_preauth_once() {
        let mut s = Session::new(UnlockMode::Confirm);
        assert!(s.handle(Event::UiShown, 0).is_empty()); // no watch yet
        assert_eq!(s.handle(Event::Presence(true), 1), vec![Action::StartPreauth { needs_tap: true }]);
        assert!(s.handle(Event::Presence(true), 2).is_empty());
    }

    #[test]
    fn leaving_cancels_preauth_and_drops_grant() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        assert_eq!(s.handle(Event::Presence(false), 1), vec![Action::CancelPreauth]);
        s.handle(Event::Presence(true), 2);
        s.handle(Event::WatchApproved { grant_id: "g".into() }, 3);
        s.handle(Event::Presence(false), 4);
        assert_eq!(s.consume("g", 5), Err(ConsumeError::NoGrant));
    }

    #[test]
    fn denied_does_not_retry_until_presence_edge() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        s.handle(Event::WatchDenied, 1);
        assert_eq!(s.state, State::LockedUiShown);
        assert!(s.handle(Event::Presence(true), 2).is_empty());
    }

    #[test]
    fn failures_back_off_then_disable_until_password() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        s.handle(Event::UnlockFailed, 10);
        assert_eq!(s.state, State::Failed { until: 40 });
        assert!(s.handle(Event::Tick, 39).is_empty());
        assert_eq!(s.handle(Event::Tick, 40), vec![Action::StartPreauth { needs_tap: true }]);
        s.handle(Event::UnlockFailed, 50);
        assert_eq!(s.state, State::Failed { until: 170 });
        s.handle(Event::Tick, 170);
        s.handle(Event::UnlockFailed, 180);
        assert_eq!(s.state, State::Disabled);
        assert!(s.handle(Event::UiShown, 200).is_empty());
        assert!(s.handle(Event::ManualUnlock, 201).is_empty());
        s.handle(Event::PasswordLogin, 300);
        assert_eq!(s.failures(), 0);
        assert!(!s.handle(Event::UiShown, 400).is_empty());
    }

    #[test]
    fn success_resets_failures() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        s.handle(Event::UnlockFailed, 1);
        s.handle(Event::Tick, 31);
        s.handle(Event::WatchApproved { grant_id: "g".into() }, 32);
        assert_eq!(s.consume("g", 33), Ok(()));
        assert_eq!(s.failures(), 0);
    }

    #[test]
    fn ui_hidden_cancels_preauth() {
        let mut s = locked_present(UnlockMode::Confirm);
        s.handle(Event::UiShown, 0);
        assert_eq!(s.handle(Event::UiHidden, 1), vec![Action::CancelPreauth]);
        assert_eq!(s.state, State::Unlocked);
    }

    #[test]
    fn hub_full_flow_and_single_use_secret() {
        let mut h = Hub::new(UnlockMode::Confirm);
        h.handle(Event::Presence(true));
        h.handle(Event::UiShown);
        assert_eq!(h.take_actions(), vec![Action::StartPreauth { needs_tap: true }]);
        assert_eq!(h.snapshot().state, "preauth_tap");
        let id = h.approve("pw".into()).unwrap();
        assert_eq!(h.take_actions(), vec![Action::NotifyGrantReady]);
        let snap = h.snapshot();
        assert_eq!((snap.state, snap.grant_id.as_deref()), ("grant_ready", Some(id.as_str())));
        assert_eq!(h.consume("nope"), Err(ConsumeError::WrongGrant));
        assert_eq!(h.consume(&id), Ok("pw".to_string()));
        assert_eq!(h.consume(&id), Err(ConsumeError::NoGrant));
    }

    #[test]
    fn hub_secret_dropped_when_grant_lost() {
        let mut h = Hub::new(UnlockMode::Confirm);
        h.handle(Event::Presence(true));
        h.handle(Event::UiShown);
        let id = h.approve("pw".into()).unwrap();
        h.handle(Event::Presence(false)); // walked away
        assert_eq!(h.consume(&id), Err(ConsumeError::NoGrant));
        assert!(h.approve("late".into()).is_none()); // no preauth running
    }
}
