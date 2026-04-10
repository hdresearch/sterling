use dto_lib::orchestrator::commit_repository::ForkRepositoryResponse;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    action::{self, Action, Branch, CommitVM},
    db::{
        ApiKeyEntity, CommitRepositoriesRepository, CommitTagsRepository, DBError,
        VMCommitsRepository,
    },
};

#[derive(Debug, Clone)]
pub struct ForkRepository {
    pub source_org: String,
    pub source_repo: String,
    pub source_tag: String,
    pub new_repo_name: Option<String>,
    pub new_tag_name: Option<String>,
    pub api_key: ApiKeyEntity,
}

impl ForkRepository {
    pub fn new(
        source_org: String,
        source_repo: String,
        source_tag: String,
        new_repo_name: Option<String>,
        new_tag_name: Option<String>,
        api_key: ApiKeyEntity,
    ) -> Self {
        Self {
            source_org,
            source_repo,
            source_tag,
            new_repo_name,
            new_tag_name,
            api_key,
        }
    }
}

#[derive(Debug, Error)]
pub enum ForkRepositoryError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("source repository or tag not found (must be public)")]
    SourceNotFound,
    #[error("target repository name already exists in your organization")]
    RepoAlreadyExists,
    #[error("failed to create VM from fork: {0}")]
    BranchFailed(String),
    #[error("failed to commit forked VM: {0}")]
    CommitFailed(String),
    #[error("internal server error")]
    InternalError,
}

impl Action for ForkRepository {
    type Response = ForkRepositoryResponse;
    type Error = ForkRepositoryError;
    const ACTION_ID: &'static str = "repository.fork";

    async fn call(self, ctx: &crate::action::ActionContext) -> Result<Self::Response, Self::Error> {
        // 1. Resolve source public ref → commit
        let source_tag = ctx
            .db
            .commit_tags()
            .resolve_public_ref(&self.source_org, &self.source_repo, &self.source_tag)
            .await?
            .ok_or(ForkRepositoryError::SourceNotFound)?;

        let source_commit = ctx
            .db
            .commits()
            .get_by_id(source_tag.commit_id)
            .await?
            .ok_or(ForkRepositoryError::SourceNotFound)?;

        // 2. Branch a VM from the public commit (Ceph CoW clone — instant).
        //    Skip the commit-level access check — we already verified access
        //    through the public repo layer via resolve_public_ref.
        let branch_result = action::call(
            Branch::by_commit(self.api_key.clone(), source_commit, None, Some(1))
                .with_skip_access_check(),
        )
        .await
        .map_err(|e| ForkRepositoryError::BranchFailed(format!("{e:?}")))?;

        let vm_response = branch_result
            .vms
            .into_iter()
            .next()
            .ok_or(ForkRepositoryError::InternalError)?;

        let vm_id: Uuid = vm_response
            .vm_id
            .parse()
            .map_err(|_| ForkRepositoryError::InternalError)?;

        // 3. Commit the forked VM (creates a snapshot owned by the forker)
        let commit_id = Uuid::new_v4();
        let commit_response = action::call(
            CommitVM::new(vm_id, commit_id, self.api_key.clone(), false, false).with_name(format!(
                "fork: {}/{}:{}",
                self.source_org, self.source_repo, self.source_tag
            )),
        )
        .await
        .map_err(|e| ForkRepositoryError::CommitFailed(format!("{e:?}")))?;

        let new_commit_id = commit_response.commit_id;

        // 4. Create the repo in the forker's org (or reuse if it exists)
        let repo_name = self
            .new_repo_name
            .unwrap_or_else(|| self.source_repo.clone());
        let tag_name = self.new_tag_name.unwrap_or_else(|| self.source_tag.clone());

        let repo = match ctx
            .db
            .commit_repositories()
            .get_by_name(self.api_key.org_id(), &repo_name)
            .await?
        {
            Some(existing) => existing,
            None => {
                let description = Some(format!(
                    "Forked from {}/{}",
                    self.source_org, self.source_repo
                ));
                ctx.db
                    .commit_repositories()
                    .insert(
                        repo_name.clone(),
                        self.api_key.org_id(),
                        self.api_key.id(),
                        description,
                    )
                    .await
                    .map_err(|e| {
                        // Could be a race condition — another fork created it
                        tracing::warn!(error = %e, "Failed to create fork repo, may already exist");
                        ForkRepositoryError::RepoAlreadyExists
                    })?
            }
        };

        // 5. Tag the new commit in the forker's repo
        ctx.db
            .commit_tags()
            .insert_with_repo(
                tag_name.clone(),
                new_commit_id,
                self.api_key.id(),
                self.api_key.org_id(),
                repo.id,
                Some(format!(
                    "Forked from {}/{}:{}",
                    self.source_org, self.source_repo, self.source_tag
                )),
            )
            .await?;

        let reference = format!("{}:{}", repo_name, tag_name);

        tracing::info!(
            source = %format!("{}/{}:{}", self.source_org, self.source_repo, self.source_tag),
            target = %reference,
            vm_id = %vm_id,
            commit_id = %new_commit_id,
            "Successfully forked repository"
        );

        Ok(ForkRepositoryResponse {
            vm_id: vm_id.to_string(),
            commit_id: new_commit_id,
            repo_name,
            tag_name,
            reference,
        })
    }
}

impl_error_response!(ForkRepositoryError,
    ForkRepositoryError::Db(_) => INTERNAL_SERVER_ERROR,
    ForkRepositoryError::SourceNotFound => NOT_FOUND,
    ForkRepositoryError::RepoAlreadyExists => CONFLICT,
    ForkRepositoryError::BranchFailed(_) => INTERNAL_SERVER_ERROR,
    ForkRepositoryError::CommitFailed(_) => INTERNAL_SERVER_ERROR,
    ForkRepositoryError::InternalError => INTERNAL_SERVER_ERROR,
);
