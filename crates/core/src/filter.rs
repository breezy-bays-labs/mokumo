/// Controls whether soft-deleted records are included in query results.
///
/// Used by repository trait methods to filter results. Defaults to
/// excluding deleted records — callers must explicitly opt in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IncludeDeleted {
    #[default]
    ExcludeDeleted,
    IncludeDeleted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_exclude_deleted() {
        assert_eq!(IncludeDeleted::default(), IncludeDeleted::ExcludeDeleted);
    }
}
