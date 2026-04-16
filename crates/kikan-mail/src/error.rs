use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("failed to connect to SMTP server: {0}")]
    ConnectFailed(String),

    #[error("SMTP authentication failed")]
    AuthFailed,

    #[error("invalid email address: {0}")]
    InvalidAddress(String),

    #[error("SMTP transport error: {0}")]
    Transport(String),

    #[error("message validation failed: {0}")]
    InvalidMessage(String),
}
