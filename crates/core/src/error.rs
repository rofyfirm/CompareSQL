use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("database: {0}")]
    Db(String),
    #[error("dibatalkan")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn parse(msg: impl Into<String>) -> Self {
        Error::Parse(msg.into())
    }
}
