//! Shared formatting utilities for CLI output.

use crate::CliError;
use serde::Serialize;

/// Pretty-print a value as JSON to stdout.
pub fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::Other(format!("JSON serialization failed: {e}")))?;
    println!("{json}");
    Ok(())
}

/// Build a horizontal separator line of the given width.
pub fn separator(width: usize) -> String {
    "\u{2500}".repeat(width)
}
