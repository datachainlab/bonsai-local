// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use risc0_zkvm::SessionStats;
use url::Url;

pub(crate) type AppState = Arc<RwLock<BonsaiState>>;

pub(crate) struct EntryWithTimestamp<T> {
    pub(crate) data: T,
    pub(crate) created_at: Instant,
}

impl<T> EntryWithTimestamp<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            created_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    Succeeded,
    Failed,
}

impl ToString for SessionStatus {
    fn to_string(&self) -> String {
        match self {
            SessionStatus::Running => "RUNNING".to_string(),
            SessionStatus::Succeeded => "SUCCEEDED".to_string(),
            SessionStatus::Failed => "FAILED".to_string(),
        }
    }
}

pub(crate) struct BonsaiState {
    pub(crate) url: Url,
    pub(crate) ttl: Duration,
    // ImageID - MemoryImage
    pub(crate) images: HashMap<String, EntryWithTimestamp<Vec<u8>>>,
    // InputID - input
    pub(crate) inputs: HashMap<String, EntryWithTimestamp<Vec<u8>>>,
    // SessionID - Status
    pub(crate) sessions: HashMap<String, EntryWithTimestamp<(SessionStatus, Option<SessionStats>)>>,
    // SessionID - Receipts
    pub(crate) receipts: HashMap<String, EntryWithTimestamp<Vec<u8>>>,
}

impl BonsaiState {
    pub(crate) fn new(url: Url, ttl: Duration) -> Self {
        Self {
            url,
            ttl,
            images: HashMap::new(),
            inputs: HashMap::new(),
            sessions: HashMap::new(),
            receipts: HashMap::new(),
        }
    }

    pub(crate) fn put_image(&mut self, image_id: String, image: Vec<u8>) -> Option<Vec<u8>> {
        self.images
            .insert(image_id, EntryWithTimestamp::new(image))
            .map(|e| e.data)
    }

    pub(crate) fn get_image(&self, image_id: impl AsRef<str>) -> Option<Vec<u8>> {
        self.images.get(image_id.as_ref()).map(|e| e.data.clone())
    }

    pub(crate) fn put_input(&mut self, input_id: String, input: Vec<u8>) -> Option<Vec<u8>> {
        self.inputs
            .insert(input_id, EntryWithTimestamp::new(input))
            .map(|e| e.data)
    }

    pub(crate) fn get_input(&self, input_id: impl AsRef<str>) -> Option<Vec<u8>> {
        self.inputs.get(input_id.as_ref()).map(|e| e.data.clone())
    }

    pub(crate) fn put_session(
        &mut self,
        session_id: String,
        status: SessionStatus,
        stats: Option<SessionStats>,
    ) -> Option<(SessionStatus, Option<SessionStats>)> {
        self.sessions
            .insert(session_id, EntryWithTimestamp::new((status, stats)))
            .map(|e| e.data)
    }

    pub(crate) fn get_session(
        &self,
        session_id: impl AsRef<str>,
    ) -> Option<&(SessionStatus, Option<SessionStats>)> {
        self.sessions.get(session_id.as_ref()).map(|e| &e.data)
    }

    pub(crate) fn put_receipt(&mut self, session_id: String, receipt: Vec<u8>) -> Option<Vec<u8>> {
        self.receipts
            .insert(session_id, EntryWithTimestamp::new(receipt))
            .map(|e| e.data)
    }

    pub(crate) fn get_receipt(&self, session_id: impl AsRef<str>) -> Option<Vec<u8>> {
        self.receipts
            .get(session_id.as_ref())
            .map(|e| e.data.clone())
    }

    pub(crate) fn get_url(&self) -> String {
        self.url.to_string().trim_end_matches("/").to_string()
    }

    pub(crate) fn cleanup_expired(&mut self) {
        let ttl = self.ttl;
        self.images.retain(|_, entry| !entry.is_expired(ttl));
        self.inputs.retain(|_, entry| !entry.is_expired(ttl));
        self.sessions.retain(|_, entry| !entry.is_expired(ttl));
        self.receipts.retain(|_, entry| !entry.is_expired(ttl));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_entry_expiration() {
        let data = vec![1, 2, 3];
        let entry = EntryWithTimestamp::new(data.clone());

        // Should not be expired immediately
        assert!(!entry.is_expired(Duration::from_millis(100)));

        // Sleep for a bit and check expiration
        sleep(Duration::from_millis(150));
        assert!(entry.is_expired(Duration::from_millis(100)));
        assert!(!entry.is_expired(Duration::from_secs(1)));
    }

    #[test]
    fn test_cleanup_expired_entries() {
        let url = Url::parse("http://localhost:8080").unwrap();
        let ttl = Duration::from_millis(100);
        let mut state = BonsaiState::new(url, ttl);

        // Add some entries
        state.put_image("image1".to_string(), vec![1, 2, 3]);
        state.put_input("input1".to_string(), vec![4, 5, 6]);
        state.put_session("session1".to_string(), SessionStatus::Running, None);
        state.put_receipt("receipt1".to_string(), vec![7, 8, 9]);

        // Verify all entries exist
        assert!(state.get_image("image1").is_some());
        assert!(state.get_input("input1").is_some());
        assert!(state.get_session("session1").is_some());
        assert!(state.get_receipt("receipt1").is_some());

        // Wait for entries to expire
        sleep(Duration::from_millis(150));

        // Add new entries that should not expire
        state.put_image("image2".to_string(), vec![10, 11, 12]);
        state.put_input("input2".to_string(), vec![13, 14, 15]);

        // Run cleanup
        state.cleanup_expired();

        // Old entries should be removed
        assert!(state.get_image("image1").is_none());
        assert!(state.get_input("input1").is_none());
        assert!(state.get_session("session1").is_none());
        assert!(state.get_receipt("receipt1").is_none());

        // New entries should still exist
        assert!(state.get_image("image2").is_some());
        assert!(state.get_input("input2").is_some());
    }

    #[test]
    fn test_cleanup_with_mixed_entries() {
        let url = Url::parse("http://localhost:8080").unwrap();
        let ttl = Duration::from_millis(200);
        let mut state = BonsaiState::new(url, ttl);

        // Add first batch of entries
        state.put_image("old_image".to_string(), vec![1, 2, 3]);
        state.put_input("old_input".to_string(), vec![4, 5, 6]);

        // Wait half the TTL
        sleep(Duration::from_millis(100));

        // Add second batch of entries
        state.put_image("new_image".to_string(), vec![7, 8, 9]);
        state.put_session("new_session".to_string(), SessionStatus::Running, None);

        // Wait for first batch to expire but not second batch
        sleep(Duration::from_millis(120));

        // Run cleanup
        state.cleanup_expired();

        // First batch should be expired and removed
        assert!(state.get_image("old_image").is_none());
        assert!(state.get_input("old_input").is_none());

        // Second batch should still exist
        assert!(state.get_image("new_image").is_some());
        assert!(state.get_session("new_session").is_some());
    }

    #[test]
    fn test_no_cleanup_when_not_expired() {
        let url = Url::parse("http://localhost:8080").unwrap();
        let ttl = Duration::from_secs(10); // Long TTL
        let mut state = BonsaiState::new(url, ttl);

        // Add entries
        state.put_image("image".to_string(), vec![1, 2, 3]);
        state.put_input("input".to_string(), vec![4, 5, 6]);
        state.put_session("session".to_string(), SessionStatus::Running, None);
        state.put_receipt("receipt".to_string(), vec![7, 8, 9]);

        // Run cleanup immediately
        state.cleanup_expired();

        // All entries should still exist
        assert!(state.get_image("image").is_some());
        assert!(state.get_input("input").is_some());
        assert!(state.get_session("session").is_some());
        assert!(state.get_receipt("receipt").is_some());
    }
}
