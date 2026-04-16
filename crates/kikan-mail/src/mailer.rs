use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use lettre::message::header::{ContentType, HeaderName, HeaderValue};
use lettre::message::{MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::address::EmailAddress;
use crate::config::{SmtpConfig, TlsMode};
use crate::error::MailError;
use crate::message::OutgoingMail;

fn parse_mailbox(addr: &EmailAddress) -> Result<lettre::message::Mailbox, MailError> {
    addr.as_str()
        .parse::<lettre::message::Mailbox>()
        .map_err(|e| MailError::InvalidAddress(e.to_string()))
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, msg: OutgoingMail) -> Result<(), MailError>;
}

pub struct LettreMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl LettreMailer {
    pub fn new(config: SmtpConfig) -> Result<Self, MailError> {
        let builder = match config.tls {
            TlsMode::Required => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                .map_err(|e| MailError::ConnectFailed(e.to_string()))?,
            TlsMode::Wrapper => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                .map_err(|e| MailError::ConnectFailed(e.to_string()))?,
            TlsMode::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host),
        };

        let mut builder = builder.port(config.port);

        if let (Some(user), Some(pass)) = (config.username, config.password) {
            builder = builder.credentials(Credentials::new(user, pass));
        }

        Ok(Self {
            transport: builder.build(),
        })
    }
}

impl std::fmt::Debug for LettreMailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LettreMailer").finish_non_exhaustive()
    }
}

pub(crate) fn build_message(msg: &OutgoingMail) -> Result<Message, MailError> {
    let mut builder = Message::builder().from(parse_mailbox(&msg.from)?);

    for addr in &msg.to {
        builder = builder.to(parse_mailbox(addr)?);
    }
    for addr in &msg.cc {
        builder = builder.cc(parse_mailbox(addr)?);
    }
    for addr in &msg.bcc {
        builder = builder.bcc(parse_mailbox(addr)?);
    }

    builder = builder.subject(&msg.subject);

    for (key, value) in &msg.headers {
        let name = HeaderName::new_from_ascii(key.clone())
            .map_err(|_| MailError::InvalidMessage(format!("invalid header name: {key}")))?;
        builder = builder.raw_header(HeaderValue::new(name, value.clone()));
    }

    match (&msg.text_body, &msg.html_body) {
        (Some(text), Some(html)) => builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .content_type(ContentType::TEXT_PLAIN)
                            .body(text.clone()),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .content_type(ContentType::TEXT_HTML)
                            .body(html.clone()),
                    ),
            )
            .map_err(|e| MailError::InvalidMessage(e.to_string())),
        (Some(text), None) => builder
            .body(text.clone())
            .map_err(|e| MailError::InvalidMessage(e.to_string())),
        (None, Some(html)) => builder
            .singlepart(
                SinglePart::builder()
                    .content_type(ContentType::TEXT_HTML)
                    .body(html.clone()),
            )
            .map_err(|e| MailError::InvalidMessage(e.to_string())),
        (None, None) => builder
            .body(String::new())
            .map_err(|e| MailError::InvalidMessage(e.to_string())),
    }
}

#[async_trait]
impl Mailer for LettreMailer {
    async fn send(&self, msg: OutgoingMail) -> Result<(), MailError> {
        let message = build_message(&msg)?;
        self.transport
            .send(message)
            .await
            .map_err(|e| MailError::Transport(e.to_string()))?;
        Ok(())
    }
}

#[derive(Default, Clone, Debug)]
pub struct CapturingMailer {
    inner: Arc<Mutex<Vec<OutgoingMail>>>,
}

impl CapturingMailer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn messages(&self) -> Vec<OutgoingMail> {
        self.inner.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

#[async_trait]
impl Mailer for CapturingMailer {
    async fn send(&self, msg: OutgoingMail) -> Result<(), MailError> {
        self.inner.lock().unwrap().push(msg);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn test_mail() -> OutgoingMail {
        OutgoingMail {
            from: EmailAddress::parse("sender@example.com").unwrap(),
            to: vec![EmailAddress::parse("dest@example.com").unwrap()],
            cc: vec![],
            bcc: vec![],
            subject: "Test".into(),
            text_body: Some("Hello".into()),
            html_body: None,
            headers: BTreeMap::new(),
        }
    }

    #[test]
    fn build_text_only_message() {
        let msg = test_mail();
        let result = build_message(&msg);
        assert!(result.is_ok());
    }

    #[test]
    fn build_html_only_message() {
        let mut msg = test_mail();
        msg.text_body = None;
        msg.html_body = Some("<p>Hello</p>".into());
        assert!(build_message(&msg).is_ok());
    }

    #[test]
    fn build_multipart_message() {
        let mut msg = test_mail();
        msg.html_body = Some("<p>Hello</p>".into());
        assert!(build_message(&msg).is_ok());
    }

    #[test]
    fn build_empty_body_message() {
        let mut msg = test_mail();
        msg.text_body = None;
        msg.html_body = None;
        assert!(build_message(&msg).is_ok());
    }

    #[test]
    fn build_message_with_cc_bcc() {
        let mut msg = test_mail();
        msg.cc = vec![EmailAddress::parse("cc@example.com").unwrap()];
        msg.bcc = vec![EmailAddress::parse("bcc@example.com").unwrap()];
        assert!(build_message(&msg).is_ok());
    }

    #[test]
    fn build_message_with_custom_headers() {
        let mut msg = test_mail();
        msg.headers
            .insert("X-Mokumo-Kind".into(), "password-reset".into());
        let message = build_message(&msg).unwrap();
        let formatted = message.formatted();
        let raw = String::from_utf8_lossy(&formatted);
        assert!(raw.contains("X-Mokumo-Kind"));
        assert!(raw.contains("password-reset"));
    }

    #[test]
    fn build_message_rejects_invalid_header_name() {
        let mut msg = test_mail();
        msg.headers.insert("Bad Header:".into(), "value".into());
        assert!(matches!(
            build_message(&msg),
            Err(MailError::InvalidMessage(_))
        ));
    }

    #[test]
    fn lettre_mailer_new_no_tls() {
        let config = SmtpConfig {
            host: "127.0.0.1".into(),
            port: 2525,
            username: None,
            password: None,
            tls: TlsMode::None,
        };
        assert!(LettreMailer::new(config).is_ok());
    }

    #[test]
    fn lettre_mailer_new_with_credentials() {
        let config = SmtpConfig {
            host: "127.0.0.1".into(),
            port: 2525,
            username: Some("user".into()),
            password: Some("pass".into()),
            tls: TlsMode::None,
        };
        assert!(LettreMailer::new(config).is_ok());
    }
}
