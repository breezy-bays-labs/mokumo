use std::fmt;

/// A profile identifier, generic over the vertical's profile discriminant.
///
/// Kikan never names concrete variants — `K` flows through opaquely from
/// the host graft's [`Graft::ProfileKind`](crate::Graft::ProfileKind). The
/// vertical supplies a concrete `K` (e.g. Mokumo's `SetupMode`) at compose
/// time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileId<K>(K);

impl<K> ProfileId<K> {
    pub fn new(kind: K) -> Self {
        Self(kind)
    }
}

impl<K: Copy> ProfileId<K> {
    pub fn get(&self) -> K {
        self.0
    }
}

impl<K> From<K> for ProfileId<K> {
    fn from(kind: K) -> Self {
        Self(kind)
    }
}

impl<K: fmt::Display> fmt::Display for ProfileId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
