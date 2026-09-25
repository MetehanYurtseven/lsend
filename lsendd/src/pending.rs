use ipc::{PendingContent, PendingEntry};
use localsend::http::server::v2::PrepareUploadDecisionV2;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Upload requests waiting for `lsendctl accept` or `decline`, shared between
/// the receiver (which adds them) and the IPC server (which decides them).
pub type SharedPending = Arc<Mutex<Pending>>;

/// What a request offers.
pub enum Content {
    /// The IDs of the offered files.
    Files(HashSet<String>),
    /// A text message; the text is the request itself.
    Text(String),
}

impl Content {
    pub fn summary(&self) -> PendingContent {
        match self {
            Self::Files(ids) => PendingContent::Files { count: ids.len() },
            Self::Text(_) => PendingContent::Text,
        }
    }
}

/// The sender withdrew the request before it was decided.
#[derive(Debug)]
pub struct Withdrawn;

pub struct PendingRequest {
    pub session_id: String,
    pub alias: String,
    pub fingerprint: String,
    pub address: String,
    pub content: Content,
    pub decision_tx: oneshot::Sender<PrepareUploadDecisionV2>,
}

impl PendingRequest {
    /// Accepts every offered file. Returns the text of a message.
    pub fn accept(self) -> Result<Option<String>, Withdrawn> {
        let (ids, text) = match self.content {
            Content::Files(ids) => (ids, None),
            // Accepting no file ends a message request with 204.
            Content::Text(text) => (HashSet::new(), Some(text)),
        };
        self.decision_tx
            .send(PrepareUploadDecisionV2::Accept(ids))
            .map_err(|_| Withdrawn)?;
        Ok(text)
    }

    pub fn decline(self) -> Result<(), Withdrawn> {
        self.decision_tx
            .send(PrepareUploadDecisionV2::Decline)
            .map_err(|_| Withdrawn)
    }
}

#[derive(Default)]
pub struct Pending {
    last_id: u64,
    requests: BTreeMap<u64, PendingRequest>,
    /// Alias of the sender for each accepted file session, for logging.
    /// Only accepted requests become sessions and end with `SessionEnd`.
    sessions: HashMap<String, String>,
}

impl Pending {
    /// Adds `request` under a fresh, short ID for `lsendctl`.
    pub fn add(&mut self, request: PendingRequest) -> u64 {
        self.last_id += 1;
        self.requests.insert(self.last_id, request);
        self.last_id
    }

    pub fn take(&mut self, id: u64) -> Option<PendingRequest> {
        self.requests.remove(&id)
    }

    /// Accepts `request`, remembering the sender of a file session until it
    /// ends. Returns the text of a message.
    pub fn accept(&mut self, request: PendingRequest) -> Result<Option<String>, Withdrawn> {
        let (session_id, alias) = (request.session_id.clone(), request.alias.clone());
        let text = request.accept()?;
        if text.is_none() {
            self.sessions.insert(session_id, alias);
        }
        Ok(text)
    }

    /// Forgets an ended session, returning the alias of its sender.
    pub fn end_session(&mut self, session_id: &str) -> Option<String> {
        self.sessions.remove(session_id)
    }

    /// Forgets the request of an aborted session.
    pub fn remove_session(&mut self, session_id: &str) {
        self.requests
            .retain(|_, request| request.session_id != session_id);
    }

    /// The waiting requests, ordered by ID.
    pub fn entries(&self) -> Vec<PendingEntry> {
        self.requests
            .iter()
            .map(|(id, request)| PendingEntry {
                id: *id,
                alias: request.alias.clone(),
                fingerprint: request.fingerprint.clone(),
                address: request.address.clone(),
                content: request.content.summary(),
            })
            .collect()
    }
}
