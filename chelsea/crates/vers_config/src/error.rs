use thiserror::Error;
use util::secrets::SecretReadError;

#[derive(Debug, Error)]
pub enum GetSourcesError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Error parsing source name: {0}")]
    ParseSourceName(#[from] ParseSourceNameError),
    #[error("Error parsing ini contents: {0}")]
    ParseIni(#[from] ParseIniError),
    #[error("Error while parsing sources.txt: {0}")]
    ParseSourcesTxt(#[from] ParseSourcesTxtError),
    #[error("Error while executing source directive: {0}")]
    ExecuteDirective(#[from] ExecuteDirectiveError),
    #[error("Unable to locate config dir at {0}")]
    ConfigDirDoesntExist(String),
    #[error("Two sources cannot share the same priority value: {0}, {1}")]
    NonUniquePriority(String, String),
}

#[derive(Debug, Error)]
pub enum ParseSourceNameError {
    #[error("Source name '{source_name}' does not contain a '-' character")]
    MissingDash { source_name: String },
    #[error("Source name '{source_name}' contains an empty description")]
    EmptyDescription { source_name: String },
    #[error(
        "Source name '{source_name}' has an invalid priority; priority must be a valid decimal number"
    )]
    InvalidPriority { source_name: String },
}

#[derive(Debug, Error)]
pub enum ParseIniError {
    #[error("line {line}: missing '=' separator")]
    MissingEquals { line: usize },
    #[error("line {line}: invalid key '{key}' (must contain only letters and underscores)")]
    InvalidKey { line: usize, key: String },
    #[error("line {line}: key cannot be empty")]
    EmptyKey { line: usize },
}

#[derive(Debug, Error)]
pub enum ParseDirectiveError {
    #[error("directive cannot be empty")]
    EmptyDirective,

    #[error("unknown directive: {directive}")]
    UnknownDirective { directive: String },

    #[error("directive '{directive}' expects {expected} arguments, found {found}")]
    InvalidArgumentCount {
        directive: String,
        expected: usize,
        found: usize,
    },
}

#[derive(Debug, Error)]
pub enum ExecuteDirectiveError {
    #[error("Error while parsing source name: {0}")]
    ParseSourceName(#[from] ParseSourceNameError),
    #[error("Error while reading AWS secret: {0}")]
    SecretRead(#[from] SecretReadError),
    #[error("Error while parsing ini: {0}")]
    ParseIni(#[from] ParseIniError),
}

#[derive(Debug, Error)]
pub enum ParseSourcesTxtError {
    #[error("Error while parsing sources.txt: line {line_number}: {error}")]
    ParseLineError {
        line_number: usize,
        error: ParseDirectiveError,
    },
}
