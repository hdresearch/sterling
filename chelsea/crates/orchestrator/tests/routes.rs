use dto_lib::chelsea_server2::vm::{
    VmCreateVmConfig, VmExecLogQuery, VmExecRequest, VmExecStreamAttachRequest,
};
use orch_test::{
    ActionTestEnv,
    client::{TestClient, TestError},
};

#[cfg(feature = "with-chelsea")]
use orchestrator::db::ChelseaNodeRepository;
use orchestrator::inbound::routes::controlplane::vm::NewRootRequest;
use uuid::Uuid;

#[test]
fn route_new_root_without_api_key() {
    ActionTestEnv::with_env(|_env| async {
        let client = TestClient::new(_env.inbound());

        let res = client
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await;

        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[cfg(feature = "with-chelsea")]
#[test]
fn route_new_root_with_api_key() {
    ActionTestEnv::with_env(|_env| async move {
        use std::time::Duration;

        use tokio::time;

        let nodes = _env
            .db()
            .node()
            .all_under_orchestrator(_env.orch.id())
            .await
            .unwrap();

        let test = TestClient::new(_env.inbound()).with_bearer(_env.orch_apikey());

        let _res = test
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await
            .expect("new root vm request failed");

        time::sleep(Duration::from_secs(2)).await;

        let vms = test.vm_list().await.expect("list vms request failed");

        assert!(
            !vms.is_empty(), // We can't check for == 1 because other tests may run
            // simultaniously
            "new_root didn't actually create any new root, even if status == ok\nlist vms endpoint result: {:?}",
            vms
        );

        test.vm_delete(&_res.vm_id)
            .await
            .expect("delete vm request failed");
    })
}

#[test]
fn route_branch_without_api_key() {
    ActionTestEnv::with_env(|env| async {
        let client = TestClient::new(env.inbound());
        let body = client.vm_branch("fake-vm-id").await;
        assert_eq!(body.unwrap_err(), TestError::Unauthorized);
    })
}

#[cfg(feature = "with-chelsea")]
#[test]
fn route_branch_with_api_key() {
    ActionTestEnv::with_env(|_env| async move {
        use std::time::Duration;

        use tokio::time;

        let test = TestClient::new(_env.inbound()).with_bearer(_env.orch_apikey());

        let res = test
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await
            .expect("new-root req failed");

        time::sleep(Duration::from_secs(4)).await;
        let branch_res = test.vm_branch(&res.vm_id).await.expect("branch-req failed");

        test.vm_delete(&res.vm_id).await.expect("delete vm failed");

        let res = test.vm_delete(&branch_res.vm_id).await;

        assert_eq!(
            res.expect_err("Being able to remove a child after removing it's parent is illegal"),
            TestError::ResourceOrRouteNotFound,
            "Being able to remove a child after removing it's parent is illegal"
        );
    })
}

#[test]
fn route_commit_without_api_key() {
    ActionTestEnv::with_env(|env| async {
        let client = TestClient::new(env.inbound());
        let res = client.vm_commit("bad-commit-it").await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[cfg(feature = "with-chelsea")]
#[test]
fn route_commit_with_api_key() {
    ActionTestEnv::with_env(|env| async {
        use std::time::Duration;

        use tokio::time;

        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let res = client
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await
            .expect("new-root req failed");

        time::sleep(Duration::from_secs(4)).await;

        let res2 = client
            .vm_commit(&res.vm_id)
            .await
            .expect("vm-commit req failed");

        let res3 = client
            .vm_from_commit(res2.commit_id.parse().unwrap())
            .await
            .expect("from-commit req failed");

        client
            .vm_delete(&res.vm_id)
            .await
            .expect("delete-vm request failed");

        time::sleep(Duration::from_secs(4)).await;

        client
            .vm_delete(&res3.vm_id)
            .await
            .expect("delete-vm request failed");
    })
}

#[test]
fn route_delete_without_api_key() {
    ActionTestEnv::with_env(|env| async {
        let client = TestClient::new(env.inbound());

        let res = client.vm_delete("fake-id").await;

        assert_eq!(
            res.expect_err("Illegal that request succeeds without api key"),
            TestError::Unauthorized,
            "Illegal that request succeeds without api key"
        );
    })
}

#[cfg(feature = "with-chelsea")]
#[test]
fn route_delete_with_api_key() {
    ActionTestEnv::with_env(|env| async {
        use std::time::Duration;

        use tokio::time;

        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let res2 = client
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await
            .expect("new-root req failed");

        time::sleep(Duration::from_secs(4)).await;

        let _direct_child1 = client
            .vm_branch(&res2.vm_id)
            .await
            .expect("vm_branch req failed");

        time::sleep(Duration::from_secs(4)).await;

        let direct_child2 = client
            .vm_branch(&res2.vm_id)
            .await
            .expect("vm_branch req failed");

        time::sleep(Duration::from_secs(4)).await;

        let _direct_child2_child = client
            .vm_branch(&direct_child2.vm_id)
            .await
            .expect("vm_branch req failed");

        client
            .vm_delete(&direct_child2.vm_id)
            .await
            .expect("vm delete operation failed");

        let err = client
            .vm_delete(&_direct_child2_child.vm_id)
            .await
            .expect_err("should not succesd");

        assert_eq!(err, TestError::ResourceOrRouteNotFound);

        client
            .vm_delete(&_direct_child1.vm_id)
            .await
            .expect("Should not fail");

        client
            .vm_delete(&res2.vm_id)
            .await
            .expect("Should not fail.");
    })
}

#[test]
fn route_ssh_key_without_api_key() {
    ActionTestEnv::with_env(|_env| async {
        let client = TestClient::new(_env.inbound());

        let res = client.ssh_key(&Uuid::new_v4().to_string()).await;

        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[cfg(feature = "with-chelsea")]
#[test]
fn route_ssh_key_with_api_key() {
    ActionTestEnv::with_env(|_env| async move {
        use std::time::Duration;

        let test = TestClient::new(_env.inbound()).with_bearer(_env.orch_apikey());

        let _res = test
            .new_root(NewRootRequest {
                vm_config: VmCreateVmConfig {
                    kernel_name: None,
                    image_name: None,
                    vcpu_count: None,
                    mem_size_mib: None,
                    fs_size_mib: None,
                },
            })
            .await
            .expect("new root vm request failed");

        tokio::time::sleep(Duration::from_secs(2)).await;

        let _ssh = test.ssh_key(&_res.vm_id).await.unwrap();

        test.vm_delete(&_res.vm_id)
            .await
            .expect("delete vm request failed");
    })
}

// ============================================================================
// Exec route tests
// ============================================================================

#[test]
fn route_exec_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound());
        let res = client
            .vm_exec(
                &Uuid::new_v4().to_string(),
                VmExecRequest {
                    command: vec!["echo".into(), "hello".into()],
                    exec_id: None,
                    env: None,
                    working_dir: None,
                    stdin: None,
                    timeout_secs: None,
                },
            )
            .await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[test]
fn route_exec_with_api_key_vm_not_found() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .vm_exec(
                &Uuid::new_v4().to_string(),
                VmExecRequest {
                    command: vec!["echo".into(), "hello".into()],
                    exec_id: None,
                    env: None,
                    working_dir: None,
                    stdin: None,
                    timeout_secs: None,
                },
            )
            .await;
        assert_eq!(res.unwrap_err(), TestError::ResourceOrRouteNotFound);
    })
}

#[test]
fn route_logs_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound());
        let res = client
            .vm_logs(&Uuid::new_v4().to_string(), &VmExecLogQuery::default())
            .await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[test]
fn route_logs_with_api_key_vm_not_found() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .vm_logs(&Uuid::new_v4().to_string(), &VmExecLogQuery::default())
            .await;
        assert_eq!(res.unwrap_err(), TestError::ResourceOrRouteNotFound);
    })
}

#[test]
fn route_exec_stream_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound());
        let res = client
            .vm_exec_stream(
                &Uuid::new_v4().to_string(),
                VmExecRequest {
                    command: vec!["ls".into()],
                    exec_id: None,
                    env: None,
                    working_dir: None,
                    stdin: None,
                    timeout_secs: None,
                },
            )
            .await
            .expect("request should not fail at transport level");
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    })
}

#[test]
fn route_exec_stream_attach_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async {
        let client = TestClient::new(env.inbound());
        let res = client
            .vm_exec_stream_attach(
                &Uuid::new_v4().to_string(),
                VmExecStreamAttachRequest {
                    exec_id: Uuid::new_v4(),
                    cursor: None,
                    from_latest: None,
                },
            )
            .await
            .expect("request should not fail at transport level");
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    })
}

// ── Env Vars ────────────────────────────────────────────────────────────────

use dto_lib::orchestrator::env_var::SetEnvVarsRequest;
use std::collections::HashMap;

#[test]
fn route_list_env_vars_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());
        let res = client.list_env_vars().await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[test]
fn route_list_env_vars_with_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client.list_env_vars().await.unwrap();
        assert!(res.vars.is_empty());
    })
}

#[test]
fn route_set_env_vars_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([("FOO".to_string(), "bar".to_string())]),
            })
            .await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[test]
fn route_set_env_vars_with_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([
                    (
                        "DATABASE_URL".to_string(),
                        "postgres://localhost".to_string(),
                    ),
                    ("API_KEY".to_string(), "secret".to_string()),
                ]),
            })
            .await
            .unwrap();
        assert_eq!(res.vars.len(), 2);
        assert_eq!(
            res.vars.get("DATABASE_URL").unwrap(),
            "postgres://localhost"
        );
        assert_eq!(res.vars.get("API_KEY").unwrap(), "secret");
    })
}

#[test]
fn route_set_env_vars_invalid_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([("1BAD".to_string(), "val".to_string())]),
            })
            .await;
        let err = res.unwrap_err();
        match err {
            TestError::FailedStatusCodeAssert { got_status, .. } => {
                assert_eq!(got_status, axum::http::StatusCode::BAD_REQUEST);
            }
            other => panic!("expected BadRequest, got: {other}"),
        }
    })
}

#[test]
fn route_delete_env_var_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());
        let res = client.delete_env_var("FOO").await;
        assert_eq!(res.unwrap_err(), TestError::Unauthorized);
    })
}

#[test]
fn route_delete_env_var_not_found() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client.delete_env_var("NONEXISTENT").await;
        assert_eq!(res.unwrap_err(), TestError::ResourceOrRouteNotFound);
    })
}

#[test]
fn route_set_then_delete_env_var() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        // Set a variable
        client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([("TO_DELETE".to_string(), "temp".to_string())]),
            })
            .await
            .unwrap();

        // Delete it
        client.delete_env_var("TO_DELETE").await.unwrap();

        // Verify it's gone
        let res = client.list_env_vars().await.unwrap();
        assert!(!res.vars.contains_key("TO_DELETE"));
    })
}

#[test]
fn route_set_env_vars_replace_removes_old_keys() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        // Set initial vars
        client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([
                    ("OLD_VAR".to_string(), "old".to_string()),
                    ("KEEP_VAR".to_string(), "keep".to_string()),
                ]),
            })
            .await
            .unwrap();

        // Replace with a completely new set — OLD_VAR should be gone
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: true,
                vars: HashMap::from([("NEW_VAR".to_string(), "new".to_string())]),
            })
            .await
            .unwrap();

        assert_eq!(res.vars.len(), 1);
        assert_eq!(res.vars.get("NEW_VAR").unwrap(), "new");
        assert!(!res.vars.contains_key("OLD_VAR"));
        assert!(!res.vars.contains_key("KEEP_VAR"));
    })
}

#[test]
fn route_set_env_vars_replace_with_empty_clears_all() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        // Set some vars
        client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([("FOO".to_string(), "bar".to_string())]),
            })
            .await
            .unwrap();

        // Replace with empty → clears everything
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: true,
                vars: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(res.vars.is_empty());
    })
}

// ── Env Vars Edge Cases ─────────────────────────────────────────────────

#[test]
fn route_set_env_vars_key_at_max_length() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let key = "A".repeat(256); // exactly at limit
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: true,
                vars: HashMap::from([(key.clone(), "val".to_string())]),
            })
            .await
            .unwrap();
        assert_eq!(res.vars.get(&key).unwrap(), "val");
    })
}

#[test]
fn route_set_env_vars_key_over_max_length() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let key = "A".repeat(257); // over limit
        let err = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([(key, "val".to_string())]),
            })
            .await
            .unwrap_err();
        match err {
            TestError::FailedStatusCodeAssert { got_status, .. } => {
                assert_eq!(got_status, axum::http::StatusCode::BAD_REQUEST);
            }
            other => panic!("expected BadRequest, got: {other}"),
        }
    })
}

#[test]
fn route_set_env_vars_value_at_max_length() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let val = "x".repeat(8192); // exactly at limit
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: true,
                vars: HashMap::from([("BIG_VAL".to_string(), val.clone())]),
            })
            .await
            .unwrap();
        assert_eq!(res.vars.get("BIG_VAL").unwrap().len(), 8192);
    })
}

#[test]
fn route_set_env_vars_value_over_max_length() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let val = "x".repeat(8193); // over limit
        let err = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::from([("BIG_VAL".to_string(), val)]),
            })
            .await
            .unwrap_err();
        match err {
            TestError::FailedStatusCodeAssert { got_status, .. } => {
                assert_eq!(got_status, axum::http::StatusCode::BAD_REQUEST);
            }
            other => panic!("expected BadRequest, got: {other}"),
        }
    })
}

#[test]
fn route_set_env_vars_empty_value_allowed() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .set_env_vars(SetEnvVarsRequest {
                replace: true,
                vars: HashMap::from([("EMPTY_VAL".to_string(), "".to_string())]),
            })
            .await
            .unwrap();
        assert_eq!(res.vars.get("EMPTY_VAL").unwrap(), "");
    })
}

#[test]
fn route_set_env_vars_empty_without_replace_rejected() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let err = client
            .set_env_vars(SetEnvVarsRequest {
                replace: false,
                vars: HashMap::new(),
            })
            .await
            .unwrap_err();
        match err {
            TestError::FailedStatusCodeAssert { got_status, .. } => {
                assert_eq!(got_status, axum::http::StatusCode::BAD_REQUEST);
            }
            other => panic!("expected BadRequest, got: {other}"),
        }
    })
}
