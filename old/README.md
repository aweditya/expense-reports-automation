# `old/` — pre-redesign workbench, slated for deletion

These files were the HTML-rendering layer for the pre-redesign pipeline.
They are physically here (as of M7.a) so the new minimal workbench
(`src/workbench_simple.rs`, M7.b) gets a clean namespace.

They are **logically still in the crate** via `#[path = "../old/..."]`
attributes on the module declarations in `src/lib.rs`, because the
old upload flow still depends on them and needs to keep working until
M8 (deletion) lands.

What's here:

- `review_workbench.rs` + `review_workbench.css` — the old workbench
  HTML renderer (~2500 lines + CSS).
- `review_fa_workbench.rs` — alternate renderer for FA-specific view.
- `review_preview.rs` — preview renderer for the final filing.
- `workbench_regressions/` — regression fixtures (rendered HTML +
  manifest.json) for the verify_workbench_regressions test.

What is **NOT** here yet but will be deleted in M8:

- The old ingestion pipeline (`ingest.rs`, `document_extract.rs`,
  `vertex_gemini.rs`, `vertex_gemini_sdk.rs`, the OCR-comparison and
  grounding modules, the synthetic corpus modules, and their export /
  verify binaries). These are still in `src/` because they're more
  deeply tangled — moving them would cascade through too many files
  for the minor namespace clarity gain.

When M8 deletes the new path's redundant ancestors, this whole
directory goes too.
