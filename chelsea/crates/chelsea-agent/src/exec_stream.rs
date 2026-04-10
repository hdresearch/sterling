use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use once_cell::sync::Lazy;
use tokio::{
    sync::{Mutex, broadcast, mpsc::UnboundedReceiver},
    time::sleep,
};
use tracing::debug;
use uuid::Uuid;

use crate::protocol::{AgentResponse, ExecStreamChunk, ExecStreamExit};

/// Maximum amount of exec stream output (in bytes) to retain in memory for reattachment.
const MAX_BACKLOG_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
/// Number of events buffered in the broadcast channel for live followers.
const BROADCAST_CAPACITY: usize = 128;
/// Duration to retain a completed exec session for reattachment.
const SESSION_RETENTION: Duration = Duration::from_secs(300);

static ACTIVE_SESSIONS: Lazy<Mutex<HashMap<Uuid, Arc<ExecStreamSession>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Events emitted by exec stream workers before being transformed into protocol responses.
pub enum ExecStreamEvent {
    Chunk(ExecStreamChunk),
    Exit(ExecStreamExit),
    Error { code: &'static str, message: String },
}

#[derive(Debug)]
pub enum SessionError {
    AlreadyExists(Uuid),
    NotFound(Uuid),
    CursorTooOld { requested: u64, available: u64 },
}

/// Manages per-exec stream state so callers can reattach to running sessions.
pub struct ExecStreamSession {
    exec_id: Uuid,
    broadcaster: broadcast::Sender<Arc<SessionEvent>>,
    state: Mutex<SessionState>,
    cleanup_scheduled: AtomicBool,
}

struct SessionState {
    backlog: VecDeque<Arc<SessionEvent>>,
    backlog_bytes: usize,
    next_cursor: u64,
    finished: bool,
}

pub struct SessionEvent {
    pub cursor: u64,
    pub response: AgentResponse,
    pub terminal: bool,
}

impl SessionEvent {
    fn size_hint(&self) -> usize {
        match &self.response {
            AgentResponse::ExecStreamChunk(chunk) => chunk.data.len(),
            _ => 256,
        }
    }
}

pub struct SessionSubscription {
    pub backlog: Vec<Arc<SessionEvent>>,
    pub receiver: broadcast::Receiver<Arc<SessionEvent>>,
}

impl ExecStreamSession {
    fn new(exec_id: Uuid) -> Arc<Self> {
        let (broadcaster, _) = broadcast::channel(BROADCAST_CAPACITY);
        Arc::new(Self {
            exec_id,
            broadcaster,
            state: Mutex::new(SessionState {
                backlog: VecDeque::new(),
                backlog_bytes: 0,
                next_cursor: 0,
                finished: false,
            }),
            cleanup_scheduled: AtomicBool::new(false),
        })
    }

    pub(crate) async fn register(
        exec_id: Uuid,
        mut rx: UnboundedReceiver<ExecStreamEvent>,
    ) -> Result<Arc<Self>, SessionError> {
        let session = {
            let mut map = ACTIVE_SESSIONS.lock().await;
            if map.contains_key(&exec_id) {
                return Err(SessionError::AlreadyExists(exec_id));
            }
            let session = ExecStreamSession::new(exec_id);
            map.insert(exec_id, session.clone());
            session
        };

        let session_clone = session.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                session_clone.push_event(event).await;
            }
            // If the channel closed without an explicit terminal event, mark as finished so cleanup runs.
            session_clone.mark_finished_if_needed().await;
        });

        Ok(session)
    }

    pub(crate) async fn get(exec_id: &Uuid) -> Result<Arc<Self>, SessionError> {
        let map = ACTIVE_SESSIONS.lock().await;
        map.get(exec_id)
            .cloned()
            .ok_or_else(|| SessionError::NotFound(*exec_id))
    }

    async fn push_event(self: &Arc<Self>, event: ExecStreamEvent) {
        match event {
            ExecStreamEvent::Chunk(mut chunk) => {
                let cursor = self.next_cursor().await;
                chunk.cursor = cursor;
                self.append_event(cursor, AgentResponse::ExecStreamChunk(chunk), false)
                    .await;
            }
            ExecStreamEvent::Exit(mut exit) => {
                let cursor = self.next_cursor().await;
                exit.cursor = cursor;
                self.append_event(cursor, AgentResponse::ExecStreamExit(exit), true)
                    .await;
            }
            ExecStreamEvent::Error { code, message } => {
                let cursor = self.next_cursor().await;
                let response = AgentResponse::error(code, message);
                self.append_event(cursor, response, true).await;
            }
        }
    }

    async fn append_event(self: &Arc<Self>, cursor: u64, response: AgentResponse, terminal: bool) {
        let event = Arc::new(SessionEvent {
            cursor,
            response,
            terminal,
        });

        let mut state = self.state.lock().await;
        state.backlog_bytes += event.size_hint();
        state.backlog.push_back(event.clone());
        while state.backlog_bytes > MAX_BACKLOG_BYTES {
            if let Some(old) = state.backlog.pop_front() {
                state.backlog_bytes = state.backlog_bytes.saturating_sub(old.size_hint());
            } else {
                break;
            }
        }

        if terminal {
            state.finished = true;
        }

        drop(state);

        if terminal {
            self.schedule_cleanup();
        }

        if let Err(err) = self.broadcaster.send(event) {
            if matches!(err, broadcast::error::SendError(_)) {
                debug!(
                    exec_id = %self.exec_id,
                    "No active listeners while broadcasting exec stream event"
                );
            }
        }
    }

    async fn mark_finished_if_needed(self: &Arc<Self>) {
        let mut state = self.state.lock().await;
        if !state.finished {
            state.finished = true;
            drop(state);
            self.schedule_cleanup();
        }
    }

    async fn next_cursor(&self) -> u64 {
        let mut state = self.state.lock().await;
        let cursor = state.next_cursor;
        state.next_cursor += 1;
        cursor
    }

    pub async fn subscribe(
        &self,
        cursor: Option<u64>,
    ) -> Result<SessionSubscription, SessionError> {
        let backlog = self.collect_backlog(cursor).await?;
        let receiver = self.broadcaster.subscribe();
        Ok(SessionSubscription { backlog, receiver })
    }

    pub async fn latest_cursor(&self) -> Option<u64> {
        let state = self.state.lock().await;
        if state.next_cursor == 0 {
            None
        } else {
            Some(state.next_cursor - 1)
        }
    }

    pub async fn collect_backlog(
        &self,
        cursor: Option<u64>,
    ) -> Result<Vec<Arc<SessionEvent>>, SessionError> {
        let state = self.state.lock().await;
        let mut results = Vec::new();
        if state.backlog.is_empty() {
            return Ok(results);
        }

        let oldest = state.backlog.front().map(|event| event.cursor).unwrap_or(0);
        if let Some(requested) = cursor {
            if requested + 1 < oldest {
                return Err(SessionError::CursorTooOld {
                    requested,
                    available: oldest,
                });
            }
        }

        for event in state.backlog.iter() {
            if cursor.map_or(true, |last| event.cursor > last) {
                results.push(event.clone());
            }
        }

        Ok(results)
    }

    fn schedule_cleanup(self: &Arc<Self>) {
        if self.cleanup_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }

        let session = Arc::clone(self);
        tokio::spawn(async move {
            sleep(SESSION_RETENTION).await;
            if session.is_finished().await {
                remove_session(&session).await;
            } else {
                session.cleanup_scheduled.store(false, Ordering::SeqCst);
            }
        });
    }

    async fn is_finished(&self) -> bool {
        let state = self.state.lock().await;
        state.finished
    }
}

async fn remove_session(session: &Arc<ExecStreamSession>) {
    let mut map = ACTIVE_SESSIONS.lock().await;
    if let Some(existing) = map.get(&session.exec_id) {
        if Arc::ptr_eq(existing, session) {
            map.remove(&session.exec_id);
            debug!(exec_id = %session.exec_id, "Removed exec stream session from registry");
        }
    }
}
