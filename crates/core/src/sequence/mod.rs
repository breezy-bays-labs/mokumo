pub mod traits;

/// A formatted sequence value combining the raw integer with its display string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedSequence {
    pub raw_value: i64,
    pub formatted: String,
}

/// Format a sequence number as `{prefix}-{value}` with zero-padding.
///
/// The value is left-padded with zeros to at least `padding` digits.
/// If the value exceeds the padding width, it naturally overflows (no error).
pub fn format_sequence_number(prefix: &str, value: i64, padding: u32) -> String {
    format!("{}-{:0>width$}", prefix, value, width = padding as usize)
}
