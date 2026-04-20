//! Kikan mailer satellite — the outbound-mail surface used by SubGraft
//! glue elsewhere in the workspace.
//!
//! Provides the [`Mailer`] trait, a [`LettreMailer`] (SMTP via
//! `lettre`) for production, and a [`CapturingMailer`] that records
//! sent messages in-memory for hermetic tests. No workspace
//! dependencies — the SubGraft wiring that registers a [`Mailer`]
//! into [`kikan::PlatformState`] lives in the consuming binary. Add a
//! new outbound mail kind by extending [`OutgoingMail`].

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
