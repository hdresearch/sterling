use tokio_postgres::{Client, Error, Statement};

/// Returns a prepared statement representing a generic `SELECT * FROM table WHERE id = $1`
pub async fn generic_stmt_fetch_by_id(
    client: &Client,
    id_col_name: &str,
    table_name: &str,
) -> Result<Statement, Error> {
    client
        .prepare(&format!(
            "SELECT * FROM {table_name} WHERE {id_col_name} = $1"
        ))
        .await
}

/// Returns a prepared statement representing a generic `DELETE FROM FROM table WHERE id = $1`
pub async fn generic_stmt_delete_by_id(
    client: &Client,
    id_col_name: &str,
    table_name: &str,
) -> Result<Statement, Error> {
    client
        .prepare(&format!(
            "DELETE FROM {table_name} WHERE {id_col_name} = $1"
        ))
        .await
}
