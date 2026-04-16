use std::collections::BTreeMap;

use cucumber::{World, given, then, when};

use kikan_mail::{CapturingMailer, EmailAddress, MailError, Mailer, OutgoingMail};

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct MailWorld {
    capturing: CapturingMailer,
    last_error: Option<MailError>,
}

impl MailWorld {
    fn new() -> Self {
        Self {
            capturing: CapturingMailer::new(),
            last_error: None,
        }
    }
}

fn make_test_mail(from: &str, to: &str, subject: &str) -> OutgoingMail {
    OutgoingMail {
        from: EmailAddress::parse(from).unwrap(),
        to: vec![EmailAddress::parse(to).unwrap()],
        cc: vec![],
        bcc: vec![],
        subject: subject.into(),
        text_body: Some("test body".into()),
        html_body: None,
        headers: BTreeMap::new(),
    }
}

// --- Background ---

#[given("a CapturingMailer instance")]
async fn given_capturing(w: &mut MailWorld) {
    w.capturing = CapturingMailer::new();
    w.last_error = None;
}

// --- When steps ---

#[rustfmt::skip]
#[when(expr = "send is called with from {string} to {string} subject {string} text_body {string} html_body {string}")]
async fn send_full(
    w: &mut MailWorld,
    from: String,
    to: String,
    subject: String,
    text_body: String,
    html_body: String,
) {
    let msg = OutgoingMail {
        from: EmailAddress::parse(from).unwrap(),
        to: vec![EmailAddress::parse(to).unwrap()],
        cc: vec![],
        bcc: vec![],
        subject,
        text_body: Some(text_body),
        html_body: Some(html_body),
        headers: BTreeMap::new(),
    };
    w.capturing.send(msg).await.unwrap();
}

#[when(expr = "send is called with header {string} equal to {string}")]
async fn send_with_header(w: &mut MailWorld, key: String, value: String) {
    let mut headers = BTreeMap::new();
    headers.insert(key, value);
    let msg = OutgoingMail {
        from: EmailAddress::parse("test@example.com").unwrap(),
        to: vec![EmailAddress::parse("dest@example.com").unwrap()],
        cc: vec![],
        bcc: vec![],
        subject: "test".into(),
        text_body: Some("body".into()),
        html_body: None,
        headers,
    };
    w.capturing.send(msg).await.unwrap();
}

#[when(expr = "send is called {int} times with the same OutgoingMail")]
async fn send_repeated(w: &mut MailWorld, count: usize) {
    for _ in 0..count {
        let msg = make_test_mail("test@example.com", "dest@example.com", "repeated");
        w.capturing.send(msg).await.unwrap();
    }
}

#[when(expr = "an EmailAddress is parsed from {string}")]
async fn parse_bad_address(w: &mut MailWorld, addr: String) {
    match EmailAddress::parse(addr) {
        Ok(_) => w.last_error = None,
        Err(e) => w.last_error = Some(e),
    }
}

// --- Then steps ---

#[then(expr = "the CapturingMailer reports {int} captured message(s)")]
async fn then_count(w: &mut MailWorld, count: usize) {
    assert_eq!(w.capturing.count(), count);
}

#[then(expr = "message {int} has from {string}")]
async fn then_from(w: &mut MailWorld, idx: usize, expected: String) {
    let msgs = w.capturing.messages();
    assert_eq!(msgs[idx].from.as_str(), expected);
}

#[then(expr = "message {int} has to {string}")]
async fn then_to(w: &mut MailWorld, idx: usize, expected: String) {
    let msgs = w.capturing.messages();
    assert_eq!(msgs[idx].to[0].as_str(), expected);
}

#[then(expr = "message {int} has subject {string}")]
async fn then_subject(w: &mut MailWorld, idx: usize, expected: String) {
    let msgs = w.capturing.messages();
    assert_eq!(msgs[idx].subject, expected);
}

#[then(expr = "message {int} has header {string} equal to {string}")]
async fn then_header(w: &mut MailWorld, idx: usize, key: String, value: String) {
    let msgs = w.capturing.messages();
    assert_eq!(msgs[idx].headers.get(&key).unwrap(), &value);
}

#[then("an InvalidAddress error is returned")]
async fn then_invalid_address(w: &mut MailWorld) {
    match &w.last_error {
        Some(MailError::InvalidAddress(_)) => {}
        other => panic!("expected InvalidAddress, got {other:?}"),
    }
}
