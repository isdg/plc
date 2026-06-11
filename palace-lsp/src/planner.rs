//! Pure planners. Each takes the current state (index/graph) plus an
//! intent, and returns a `WorkspaceEdit` describing the change. No I/O,
//! no global mutation — easy to unit-test.
//!
//! The LSP handler hands the returned edit to `client.apply_edit`;
//! the resulting `did_change` notifications drive the actual graph
//! mutation. One writer, no drift.

use serde_json::Value;
use tower_lsp::lsp_types::WorkspaceEdit;

use crate::index::Index;

pub fn plan_create_note(
    _idx: &Index,
    _args: &[Value],
) -> Option<WorkspaceEdit> {
    // TODO: emit CreateFile + seed front-matter/title; optionally an
    // edit on the active buffer that inserts [[new-note]] at cursor.
    None
}

pub fn plan_delete_note(
    _idx: &Index,
    _args: &[Value],
) -> Option<WorkspaceEdit> {
    // TODO: gather backlinkers from graph, emit a TextDocumentEdit per
    // backlinker that scrubs `[[target]]`, then a DeleteFile op.
    None
}

pub fn plan_link(
    _idx: &Index,
    _args: &[Value],
) -> Option<WorkspaceEdit> {
    // TODO: insert `[[target]]` into the source note (at cursor or in
    // a "Links" section), respecting normalize_target() conventions
    // already used by palace-core.
    None
}

pub fn plan_unlink(
    _idx: &Index,
    _args: &[Value],
) -> Option<WorkspaceEdit> {
    // TODO: locate and remove the `[[target]]` occurrence in the
    // source note. Decide policy on multiple occurrences.
    None
}
