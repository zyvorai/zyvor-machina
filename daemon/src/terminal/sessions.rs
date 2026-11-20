// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::RngCore;

#[derive(Debug, Clone)]
pub struct PendingSession {
    pub host: String,
    pub ssh_user: String,
    pub ssh_port: u16,
    pub created_by: String,
    pub created_at: Instant,
}

#[derive(Clone)]
pub struct TerminalSessionStore {
    inner: Arc<Mutex<HashMap<String, PendingSession>>>,
}

impl TerminalSessionStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn purge_expired(&self, ttl: Duration) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        g.retain(|_, v| v.created_at.elapsed() <= ttl);
    }

    /// Insert a new pending session and return its opaque id (hex).
    pub fn insert_session(
        &self,
        host: String,
        ssh_user: String,
        ssh_port: u16,
        created_by: String,
        ttl: Duration,
    ) -> String {
        self.purge_expired(ttl);
        let id: String = {
            let mut b = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut b);
            hex::encode(b)
        };
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.insert(
            id.clone(),
            PendingSession {
                host,
                ssh_user,
                ssh_port,
                created_by,
                created_at: Instant::now(),
            },
        );
        id
    }

    /// Remove and return a session if it exists and is still within `ttl` of creation.
    pub fn take(&self, id: &str, ttl: Duration) -> Option<PendingSession> {
        self.purge_expired(ttl);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let s = g.remove(id)?;
        if s.created_at.elapsed() > ttl {
            return None;
        }
        Some(s)
    }
}
