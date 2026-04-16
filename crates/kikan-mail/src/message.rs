use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::address::EmailAddress;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMail {
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub headers: BTreeMap<String, String>,
}
