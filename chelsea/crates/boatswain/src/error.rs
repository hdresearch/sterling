use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub enum Error {
    IoError(String),
    DockerBuild(String),
    DockerCreate(String),
    DockerExport(String),
    Tar(String),
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => e.fmt(f),
            Self::DockerBuild(e) => write!(f, "error building docker image: {e}"),
            Self::DockerCreate(e) => write!(f, "error creating docker container: {e}"),
            Self::DockerExport(e) => write!(f, "error exporting docker container: {e}"),
            Self::Tar(e) => write!(f, "error executing tar: {e}"),
        }
    }
}
