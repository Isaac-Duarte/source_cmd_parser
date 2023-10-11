use std::path::PathBuf;

pub type SourceCmdResult<T = ()> = std::result::Result<T, SourceCmdError>;

#[derive(Debug, thiserror::Error)]
pub enum SourceCmdError {
    #[error("Missing Field in builder: {0}")]
    MissingFieldS(String),

    #[error("Unable to get parent directory for {0}")]
    UnableToGetParentDirectory(PathBuf),

    #[error(transparent)]
    RegexError(#[from] regex::Error),

    #[error(transparent)]
    NotifyError(#[from] notify::Error),

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}
