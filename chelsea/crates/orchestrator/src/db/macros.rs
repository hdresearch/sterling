// NOTE ABOUT `cfg(test)` BEHAVIOR IN THIS FILE
// -------------------------------------------
// These macros wrap common Postgres operations and (in non-test builds) cache
// prepared statements in a static OnceCell. That caching is desirable in
// production because:
// - Preparing statements only once saves a round trip and reduces server work.
// - The statement handle can be reused safely on the same connection.
//
// However, during integration tests we often:
// - Open fresh connections (new Client instances) per test, and
// - Start/ROLLBACK transactions or drop/recreate schemas between tests.
//
// Postgres prepared statements are bound to the specific server-side session
// (i.e., the connection). If we cache a prepared statement handle in a static
// OnceCell and then use it with a different connection (common in tests), we
// hit runtime errors like:
//   "prepared statement 'sN' does not exist" (SQLSTATE E26000)
// because the cached handle refers to a statement created in a different
// session.
//
// To avoid this, under `cfg(test)` we skip the global cache and prepare the
// statement per call. This keeps tests reliable while leaving production code
// unchanged (still using OnceCell caching).
//
// Summary:
// - Non-test: cache prepared statements via OnceCell → better perf.
// - Test: prepare per call → avoids cross-connection handle reuse.
//
// If you prefer this behavior to be controlled via a feature flag instead of
// `cfg(test)`, we can swap the condition accordingly.

// @vincent-thomas: When having a pool of db connections, preparing statements is trickier. I've
// commented it out for now.

macro_rules! execute_sql {
    ($self:expr, $query:expr, $types:expr, $params:expr) => {{
        // For unused lints.
        let _: &[tokio_postgres::types::Type] = $types;
        tracing::debug!("executing statement: {}", $query);
        match $self.raw_obj().await.execute($query, $params).await {
           Ok(value) => Ok(value),
           Err(err) => {
             tracing::error!(err = ?&err, "db query error");
             Err(err)
           }
        }
    }};

    ($self:expr, $query:expr) => {
        execute_sql!($self, $query, &[], &[])
    };
}

macro_rules! query_sql {
    ($self:expr, $query:expr, $types:expr, $params:expr) => {{
        let obj = $self.raw_obj().await;
        // For unused lints.
        let _: &[tokio_postgres::types::Type] = $types;
        tracing::debug!("executing statement: {}", $query);
        match obj.query($query, $params).await {
           Ok(value) => Ok(value),
           Err(err) => {
             tracing::error!(err = ?&err, "db query error");
             Err(err)
           }
        }
    }};

    ($self:expr, $query:expr) => {
        query_sql!($self, $query, &[], &[])
    };
}

macro_rules! query_one_sql {
    ($self:expr, $query:expr, $types:expr, $params:expr) => {{
        match query_sql!($self, $query, $types, $params) {
            Ok(result) => match result.get(0) {
                Some(row) => Ok(Some(row.clone())),
                None => Ok(None),
            },
            Err(err) => Err(err),
        }
    }};

    ($self:expr, $query:expr) => {
        query_one_sql!($self, $query, &[], &[])
    };
}
