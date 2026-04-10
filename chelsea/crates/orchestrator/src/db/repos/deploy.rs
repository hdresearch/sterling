use tokio_postgres::types::Type;
use uuid::Uuid;

use crate::db::{DB, DBError};

// =============================================================================
// GitHub App Installations (vers_landing schema)
// =============================================================================

#[derive(Debug, Clone)]
pub struct GitHubInstallationEntity {
    pub id: Uuid,
    pub installation_id: i64,
    pub org_id: Uuid,
}

// =============================================================================
// GitHub App Repositories (vers_landing schema)
// =============================================================================

#[derive(Debug, Clone)]
pub struct GitHubRepoEntity {
    pub id: Uuid,
    pub installation_id: Uuid,
    pub github_repo_id: i64,
    pub github_repo_full_name: String,
    pub github_repo_name: String,
    pub github_repo_private: bool,
    pub github_repo_default_branch: String,
}

// =============================================================================
// Trait
// =============================================================================

pub trait DeployRepository {
    /// Find a GitHub repo by `owner/repo` full name within an org.
    fn find_repo_by_full_name_and_org(
        &self,
        full_name: &str,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Option<GitHubRepoEntity>, DBError>>;

    /// Find the GitHub App installation for an org.
    fn find_installation_by_org(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Option<GitHubInstallationEntity>, DBError>>;

    /// Insert a project row into vers_landing.projects.
    fn insert_project(
        &self,
        project_id: Uuid,
        org_id: Uuid,
        name: &str,
        root_commit_id: Uuid,
        current_vm_id: Uuid,
        created_by: Uuid,
        install_command: Option<&str>,
        build_command: Option<&str>,
        run_command: Option<&str>,
        working_directory: Option<&str>,
        github_repository_id: Option<Uuid>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Update the github_clone_status for a project.
    fn update_project_clone_status(
        &self,
        project_id: Uuid,
        status: &str,
        error: Option<&str>,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Insert a project API key mapping.
    fn insert_project_api_key(
        &self,
        project_id: Uuid,
        api_key_id: Uuid,
        api_key: &str,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Update project's github_repository_id and set clone status to pending.
    fn update_project_github_repo(
        &self,
        project_id: Uuid,
        github_repository_id: Uuid,
    ) -> impl Future<Output = Result<(), DBError>>;

    /// Delete a project and its API key mappings (rollback on failure).
    fn delete_project(&self, project_id: Uuid) -> impl Future<Output = Result<(), DBError>>;
}

pub struct Deploy(DB);

impl DB {
    pub fn deploy(&self) -> Deploy {
        Deploy(self.clone())
    }
}

impl DeployRepository for Deploy {
    async fn find_repo_by_full_name_and_org(
        &self,
        full_name: &str,
        org_id: Uuid,
    ) -> Result<Option<GitHubRepoEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT r.id, r.installation_id, r.github_repo_id, r.github_repo_full_name,
                    r.github_repo_name, r.github_repo_private, r.github_repo_default_branch
             FROM vers_landing.github_app_repositories r
             JOIN vers_landing.github_app_installations i ON r.installation_id = i.id
             WHERE r.github_repo_full_name = $1
               AND i.org_id = $2
               AND i.suspended_at IS NULL
             LIMIT 1",
            &[Type::TEXT, Type::UUID],
            &[&full_name, &org_id]
        )?;

        Ok(maybe_row.map(|row| GitHubRepoEntity {
            id: row.get("id"),
            installation_id: row.get("installation_id"),
            github_repo_id: row.get("github_repo_id"),
            github_repo_full_name: row.get("github_repo_full_name"),
            github_repo_name: row.get("github_repo_name"),
            github_repo_private: row.get("github_repo_private"),
            github_repo_default_branch: row.get("github_repo_default_branch"),
        }))
    }

    async fn find_installation_by_org(
        &self,
        org_id: Uuid,
    ) -> Result<Option<GitHubInstallationEntity>, DBError> {
        let maybe_row = query_one_sql!(
            self.0,
            "SELECT id, installation_id, org_id
             FROM vers_landing.github_app_installations
             WHERE org_id = $1
               AND suspended_at IS NULL
             LIMIT 1",
            &[Type::UUID],
            &[&org_id]
        )?;

        Ok(maybe_row.map(|row| GitHubInstallationEntity {
            id: row.get("id"),
            installation_id: row.get("installation_id"),
            org_id: row.get("org_id"),
        }))
    }

    async fn insert_project(
        &self,
        project_id: Uuid,
        org_id: Uuid,
        name: &str,
        root_commit_id: Uuid,
        current_vm_id: Uuid,
        created_by: Uuid,
        install_command: Option<&str>,
        build_command: Option<&str>,
        run_command: Option<&str>,
        working_directory: Option<&str>,
        github_repository_id: Option<Uuid>,
    ) -> Result<(), DBError> {
        let clone_status: Option<&str> = if github_repository_id.is_some() {
            Some("pending")
        } else {
            None
        };

        execute_sql!(
            self.0,
            "INSERT INTO vers_landing.projects
                (project_id, org_id, name, root_commit_id, current_vm_id, created_by,
                 install_command, build_command, run_command, working_directory,
                 github_clone_status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &[
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::UUID,
                Type::UUID,
                Type::UUID,
                Type::TEXT,
                Type::TEXT,
                Type::TEXT,
                Type::TEXT,
                Type::TEXT,
            ],
            &[
                &project_id,
                &org_id,
                &name,
                &root_commit_id,
                &current_vm_id,
                &created_by,
                &install_command,
                &build_command,
                &run_command,
                &working_directory,
                &clone_status,
            ]
        )?;
        Ok(())
    }

    async fn update_project_clone_status(
        &self,
        project_id: Uuid,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE vers_landing.projects
             SET github_clone_status = $2,
                 github_clone_error = $3,
                 github_last_sync_at = CASE WHEN $2 = 'completed' THEN NOW() ELSE github_last_sync_at END,
                 updated_at = NOW()
             WHERE project_id = $1",
            &[Type::UUID, Type::TEXT, Type::TEXT],
            &[&project_id, &status, &error]
        )?;
        Ok(())
    }

    async fn insert_project_api_key(
        &self,
        project_id: Uuid,
        api_key_id: Uuid,
        api_key: &str,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "INSERT INTO vers_landing.project_api_keys (project_id, api_key_id, api_key)
             VALUES ($1, $2, $3)",
            &[Type::UUID, Type::UUID, Type::TEXT],
            &[&project_id, &api_key_id, &api_key]
        )?;
        Ok(())
    }

    async fn update_project_github_repo(
        &self,
        project_id: Uuid,
        github_repository_id: Uuid,
    ) -> Result<(), DBError> {
        execute_sql!(
            self.0,
            "UPDATE vers_landing.projects
             SET github_repository_id = $2,
                 github_clone_status = 'pending',
                 updated_at = NOW()
             WHERE project_id = $1",
            &[Type::UUID, Type::UUID],
            &[&project_id, &github_repository_id]
        )?;
        Ok(())
    }

    async fn delete_project(&self, project_id: Uuid) -> Result<(), DBError> {
        // Delete project API keys first (FK dependency)
        execute_sql!(
            self.0,
            "DELETE FROM vers_landing.project_api_keys WHERE project_id = $1",
            &[Type::UUID],
            &[&project_id]
        )?;
        execute_sql!(
            self.0,
            "DELETE FROM vers_landing.projects WHERE project_id = $1",
            &[Type::UUID],
            &[&project_id]
        )?;
        Ok(())
    }
}
