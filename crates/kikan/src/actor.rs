use std::fmt;

/// Who is performing a mutation. Threaded from the handler layer
/// through services and into repository adapters for activity logging.
#[derive(Debug, Clone)]
pub struct Actor {
    id: String,
    actor_type: ActorType,
}

/// The kind of actor performing an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorType {
    User,
    System,
    ApiKey,
}

impl fmt::Display for ActorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::System => write!(f, "system"),
            Self::ApiKey => write!(f, "api_key"),
        }
    }
}

impl Actor {
    /// Build an actor from an authenticated user's ID.
    pub fn user(id: impl ToString) -> Self {
        Self {
            id: id.to_string(),
            actor_type: ActorType::User,
        }
    }

    /// System actor for migrations, CLI operations, and tests.
    pub fn system() -> Self {
        Self {
            id: "system".to_string(),
            actor_type: ActorType::System,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn actor_type(&self) -> ActorType {
        self.actor_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_actor_stores_id_and_type() {
        let actor = Actor::user(42);
        assert_eq!(actor.id(), "42");
        assert_eq!(actor.actor_type(), ActorType::User);
    }

    #[test]
    fn system_actor_has_system_id() {
        let actor = Actor::system();
        assert_eq!(actor.id(), "system");
        assert_eq!(actor.actor_type(), ActorType::System);
    }

    #[test]
    fn actor_type_display() {
        assert_eq!(ActorType::User.to_string(), "user");
        assert_eq!(ActorType::System.to_string(), "system");
        assert_eq!(ActorType::ApiKey.to_string(), "api_key");
    }
}
