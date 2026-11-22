---
applyTo: "**/*.md,docs/**"
---

# Documentation

- README files are rendered on crates.io and docs.rs. Every link to a repo file and every
  image uses an absolute URL (`https://github.com/...`, `https://raw.githubusercontent.com/...`).
  Only in-page anchors may be relative.
- Mermaid diagrams (```` ```mermaid ````) must be valid (`task lint:mermaid` renders them)
  and must match the code. Flag diagrams that the change makes stale. Keep each diagram under
  about 15 nodes.
- Code blocks are tagged with their language. `README.md` is included by the private
  `readme_doctests` module in `src/lib.rs`, so its `rust` blocks run as doctests and must
  compile. Use `rust,no_run` or `rust,ignore` only with a stated reason.
- Keep badges, the License section and the MSRV statement consistent with `Cargo.toml`.
