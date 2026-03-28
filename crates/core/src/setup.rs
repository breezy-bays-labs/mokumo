use serde::{Deserialize, Serialize};

/// The operational mode of the Mokumo instance.
///
/// Controls startup behavior, auth requirements, and reset eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupMode {
    Demo,
    Production,
}

impl SetupMode {
    /// Return the string representation used in the `active_profile` file
    /// and the `settings` table.
    pub fn as_str(&self) -> &'static str {
        match self {
            SetupMode::Demo => "demo",
            SetupMode::Production => "production",
        }
    }
}

impl std::fmt::Display for SetupMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SetupMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "demo" => Ok(SetupMode::Demo),
            "production" => Ok(SetupMode::Production),
            other => Err(format!("unknown setup mode: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_display_parse() {
        for mode in [SetupMode::Demo, SetupMode::Production] {
            let s = mode.to_string();
            let parsed: SetupMode = s.parse().unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!("Demo".parse::<SetupMode>().unwrap(), SetupMode::Demo);
        assert_eq!(
            "PRODUCTION".parse::<SetupMode>().unwrap(),
            SetupMode::Production
        );
    }

    #[test]
    fn parse_unknown_fails() {
        assert!("unknown".parse::<SetupMode>().is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let mode = SetupMode::Demo;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""demo""#);
        let restored: SetupMode = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, mode);
    }
}
