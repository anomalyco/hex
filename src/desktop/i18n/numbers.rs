//! Number formatting that follows the interface language.

use super::{FRENCH_COLUMN, PORTUGUESE_COLUMN, RUSSIAN_COLUMN, column_for_code, current};

/// Whether the active language writes a decimal fraction with a comma:
/// French, German, Portuguese, Russian, and Ukrainian.
pub(crate) fn decimal_comma() -> bool {
    let column = current();
    column == column_for_code("de")
        || column == column_for_code("uk")
        || column == FRENCH_COLUMN
        || column == PORTUGUESE_COLUMN
        || column == RUSSIAN_COLUMN
}
