/// Implement `IntoResponse` for an error type by mapping each variant to an HTTP status code.
///
/// Usage:
/// ```ignore
/// impl_error_response!(MyError,
///     Db(_) => INTERNAL_SERVER_ERROR,
///     NotFound => NOT_FOUND,
///     Forbidden => FORBIDDEN,
/// );
/// ```
macro_rules! impl_error_response {
    ($error:ty, $( $pat:pat => $status:ident ),+ $(,)?) => {
        impl ::axum::response::IntoResponse for $error {
            fn into_response(self) -> ::axum::response::Response {
                let status = match &self {
                    $( $pat => ::axum::http::StatusCode::$status, )+
                };
                let body = ::dto_lib::ErrorResponse::new(self.to_string());
                (status, body).into_response()
            }
        }
    };
}

mod authz;
mod base_images;
mod commits;
pub mod deploy;
mod domains;
mod env_vars;
mod keys;
mod nodes;
mod rechoose_node;
mod repositories;
mod tags;
mod telemetry_pull;
pub mod vms;

pub use authz::*;
pub use base_images::*;
pub use commits::*;
pub use deploy::{DeployError, DeployFromGitHub, DeployRequest, DeployResponse, DeploySettings};
pub use domains::*;
pub use env_vars::*;
pub use keys::*;
pub use nodes::*;
use orch_wg::WG;
pub use rechoose_node::*;
pub use repositories::*;
pub use tags::*;
pub use telemetry_pull::*;
use vers_config::VersConfig;
pub use vms::*;

use parking_lot::RwLock;
use std::{
    fmt::Debug,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::oneshot::{self, Sender},
    time,
};
use tracing::Instrument;
use vers_pg::db::VersPg;

use crate::{
    db::{DB, OrchestratorEntity},
    outbound::node_proto::ChelseaProto,
};

pub struct ActionContext {
    proto: Option<ChelseaProto>,
    pub orch: OrchestratorEntity,
    wg: Option<WG>,
    pub db: DB,
    pub vers_pg: Arc<VersPg>,
    action_timeout: Duration,
}

impl ActionContext {
    pub fn new(wg: WG, db: DB, orch: OrchestratorEntity, vers_pg: Arc<VersPg>) -> Self {
        Self {
            proto: Some(ChelseaProto::new(wg.clone())),
            wg: Some(wg),
            db,
            orch,
            vers_pg,
            action_timeout: Duration::from_secs(VersConfig::orchestrator().action_timeout_secs),
        }
    }

    /// Create a context with only DB access. Useful for testing actions
    /// that don't need WireGuard or node communication.
    #[cfg(any(test, feature = "integration-tests"))]
    pub fn db_only(db: DB, orch: OrchestratorEntity, vers_pg: Arc<VersPg>) -> Self {
        Self {
            proto: None,
            wg: None,
            db,
            orch,
            vers_pg,
            action_timeout: Duration::from_secs(30),
        }
    }

    /// Access the node communication protocol. Panics if the context was
    /// created without WireGuard (e.g. via `db_only()`).
    pub fn proto(&self) -> &ChelseaProto {
        self.proto
            .as_ref()
            .expect("ActionContext::proto() called on a db-only context (no WireGuard)")
    }

    /// Access the WireGuard interface. Panics if the context was
    /// created without WireGuard (e.g. via `db_only()`).
    pub fn wg(&self) -> &WG {
        self.wg
            .as_ref()
            .expect("ActionContext::wg() called on a db-only context (no WireGuard)")
    }
}

pub trait Action {
    type Response;
    type Error: Debug;

    /// Used for logging and ID.
    const ACTION_ID: &'static str;

    fn call(
        self,
        ctx: &'static ActionContext,
    ) -> impl Future<Output = Result<Self::Response, Self::Error>> + Send;
}

#[derive(Error, Debug)]
pub enum ActionError<E> {
    #[error("action error: {0:?}")]
    Error(E),
    #[error("timeout error")]
    Timeout,
    #[error("action panicked")]
    Panic,
    #[error("action cannot execute shutdown has been requested")]
    Shutdown,
}

impl<E> ActionError<E> {
    pub fn try_extract_err(self) -> Option<E> {
        match self {
            ActionError::Error(e) => Some(e),
            _ => None,
        }
    }
}

static ACTION_CONTEXT: OnceLock<Option<OuterActionContext>> = OnceLock::new();

pub struct OuterActionContext {
    ctx: ActionContext,
    action_count: AtomicU64,
    ctrl_c_wait_sender: RwLock<Option<Sender<()>>>,
}

/// Returns true if set before.
pub fn setup(wg: WG, db: DB, orch: OrchestratorEntity, vers_pg: Arc<VersPg>) {
    setup_with_context(ActionContext::new(wg, db, orch, vers_pg));
}

/// Set up the action system with only DB access (no WireGuard).
/// Actions that require node communication will panic if called.
#[cfg(any(test, feature = "integration-tests"))]
pub fn setup_db_only(db: DB, orch: OrchestratorEntity, vers_pg: Arc<VersPg>) {
    setup_with_context(ActionContext::db_only(db, orch, vers_pg));
}

fn setup_with_context(ctx: ActionContext) {
    let result = ACTION_CONTEXT.set(Some(OuterActionContext {
        ctx,
        action_count: AtomicU64::new(0),
        ctrl_c_wait_sender: RwLock::new(None),
    }));

    if result.is_err() {
        tracing::error!("action::setup called more than once");
    }
}

#[tracing::instrument]
pub async fn graceful_teardown() {
    tracing::info!("initiating");
    let outer_ctx = ACTION_CONTEXT
        .get()
        .expect("'crate::action::graceful_teardown' executed before 'crate::action::setup'")
        .as_ref()
        .expect("'crate::action::graceful_teardown' was called twice.");

    let (sender, receiver) = oneshot::channel();

    let mut _lock = outer_ctx.ctrl_c_wait_sender.write();

    if outer_ctx.action_count.load(Ordering::Acquire) != 0 {
        assert!(
            _lock.replace(sender).is_none(),
            "'crate::action::graceful_teardown' was called twice."
        );
        drop(_lock);
        if let Err(err) = receiver.await {
            tracing::warn!(?err, "action graceful shutdown receiver wait error");
        }
    }

    // Because of rust's stupid design implementation that static's don't actuate Drop impl
    if let Err(old) = ACTION_CONTEXT.set(None) {
        drop(old);
    };
    tracing::info!("done");
}

/// Access the shared `ActionContext` for operations that don't fit the
/// `Action` trait (e.g. streaming responses that outlive the action timeout).
///
/// Panics if `setup()` has not been called.
pub fn context() -> &'static ActionContext {
    &ACTION_CONTEXT
        .get()
        .expect("'crate::action::context' executed before 'crate::action::setup'")
        .as_ref()
        .expect("'crate::action::context' called after shutdown")
        .ctx
}

/// Call any action. The action runs inline in the current task with a timeout.
///
/// Graceful shutdown of action execution is implemented, provided the user
/// calls `crate::action::graceful_teardown` and awaits the future it returns.
pub async fn call<A>(action: A) -> Result<A::Response, ActionError<A::Error>>
where
    A: Action + Send + 'static,
    A::Response: Send,
    A::Error: Send,
{
    let outer_ctx = ACTION_CONTEXT
        .get()
        .expect("'crate::action::call' executed before 'crate::action::setup'")
        .as_ref()
        .expect("'crate::action::call' executed after shutdown");

    if outer_ctx.ctrl_c_wait_sender.read().is_some() {
        return Err(ActionError::Shutdown);
    }

    outer_ctx.action_count.fetch_add(1, Ordering::AcqRel);

    let action_fut = action
        .call(&outer_ctx.ctx)
        .instrument(tracing::debug_span!("action execution", id = A::ACTION_ID,));

    let result = time::timeout(outer_ctx.ctx.action_timeout, action_fut).await;

    let output = match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            tracing::warn!(action_id = A::ACTION_ID, ?err, "action returned error");
            Err(ActionError::Error(err))
        }
        Err(_timeout) => {
            tracing::warn!(action_id = A::ACTION_ID, "action timeout error");
            Err(ActionError::Timeout)
        }
    };

    let prev = outer_ctx.action_count.fetch_sub(1, Ordering::AcqRel);
    if prev == 1 {
        let mut sender = outer_ctx.ctrl_c_wait_sender.write();
        if let Some(reporter) = sender.take() {
            drop(sender);
            let _ = reporter.send(());
        }
    }

    output
}
