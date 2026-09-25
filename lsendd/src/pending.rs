use ipc::{PendingContent, PendingEntry};
use localsend::http::server::v2::PrepareUploadDecisionV2;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// The upload request waiting for `lsendctl accept` or `decline`, shared
/// between the receiver (which sets it) and the IPC server (which decides it).
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

/// At most one request waits at a time: it holds the server's only session
/// slot, so further senders get 409 until it is decided.
#[derive(Default)]
pub struct Pending {
    request: Option<PendingRequest>,
    /// Alias of the sender for each accepted file session, for logging.
    /// Only accepted requests become sessions and end with `SessionEnd`.
    sessions: HashMap<String, String>,
}

impl Pending {
    pub fn set(&mut self, request: PendingRequest) {
        self.request = Some(request);
    }

    /// Takes the waiting request. With `from`, only if its sender has that
    /// fingerprint, so a request that replaced the one the user looked at is
    /// not decided by mistake.
    pub fn take(&mut self, from: Option<&str>) -> Result<PendingRequest, String> {
        let request = self.request.as_ref().ok_or("No pending request")?;
        if let Some(from) = from
            && !request.fingerprint.eq_ignore_ascii_case(from)
        {
            return Err(format!(
                "The pending request is from {} ({}), not {from}",
                request.alias, request.fingerprint
            ));
        }
        Ok(self.request.take().unwrap())
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

    /// Takes the waiting request if it belongs to `session_id`, e.g. when
    /// that session was aborted or timed out.
    pub fn take_session(&mut self, session_id: &str) -> Option<PendingRequest> {
        self.request
            .take_if(|request| request.session_id == session_id)
    }

    pub fn entry(&self) -> Option<PendingEntry> {
        self.request.as_ref().map(|request| PendingEntry {
            alias: request.alias.clone(),
            fingerprint: request.fingerprint.clone(),
            address: request.address.clone(),
            content: request.content.summary(),
        })
    }
}
