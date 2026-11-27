#![allow(dead_code)]
use std::{
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A private tmux server, killed on drop.
pub struct Tmux(pub String);

impl Tmux {
    pub fn new() -> Tmux {
        Tmux(format!("wi-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)))
    }
    pub fn run(&self, args: &[&str]) {
        let status = Command::new("tmux").args(["-L", &self.0]).args(args).status().unwrap();
        assert!(status.success(), "tmux {args:?}");
    }
    pub fn has(&self, session: &str) -> bool {
        Command::new("tmux").args(["-L", &self.0, "has-session", "-t", &format!("={session}")]).status().unwrap().success()
    }
    pub fn session_names(&self) -> Vec<String> {
        let out = Command::new("tmux").args(["-L", &self.0, "list-sessions", "-F", "#{session_name}"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux").args(["-L", &self.0, "kill-server"]).status();
    }
}

/// Polls `condition` for up to 10 seconds.
pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}
