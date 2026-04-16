pub mod address;
pub mod config;
pub mod error;
pub mod mailer;
pub mod message;

pub use address::EmailAddress;
pub use config::{SmtpConfig, TlsMode};
pub use error::MailError;
pub use mailer::{CapturingMailer, LettreMailer, Mailer};
pub use message::OutgoingMail;
