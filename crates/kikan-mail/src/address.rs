use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::MailError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailAddress(String);

impl EmailAddress {
    pub fn parse(s: impl Into<String>) -> Result<Self, MailError> {
        let s: String = s.into();
        match s.parse::<lettre::Address>() {
            Ok(_) => Ok(Self(s)),
            Err(_) => Err(MailError::InvalidAddress(s)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
