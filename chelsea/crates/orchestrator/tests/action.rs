use std::{convert::Infallible, time::Duration};

use futures_util::FutureExt;
use orch_test::ActionTestEnv;
use orchestrator::action::{self, Action};
use tokio::{task, time::timeout};

pub struct MockAction;

impl Action for MockAction {
    type Error = Infallible;
    type Response = ();
    const ACTION_ID: &'static str = "mock.action";
    async fn call(
        self,
        _ctx: &'static action::ActionContext,
    ) -> Result<Self::Response, Self::Error> {
        Ok(())
    }
}

#[test]
fn try_overwhelm_or_freeze_action_shutdown_behavior() {
    ActionTestEnv::with_env(|_env| {
        timeout(Duration::from_secs(4), async move {
            async fn exec() {
                for _ in 0..1000 {
                    let _ = action::call(MockAction).await;
                }
            }

            let _ = tokio::join!(
                task::spawn(exec()),
                task::spawn(exec()),
                task::spawn(exec()),
                task::spawn(exec())
            );
        })
        .map(|e| e.unwrap())
    })
}
