---
applyTo: "**/*.rs"
---

# Rust code

- Every module (`//!`), type, field, variant, function, constant and macro has a doc
  comment, private and `pub(crate)` items included. Docs state intent, invariants, units and
  error/panic conditions (`# Errors`, `# Panics`, `# Safety`). They do not restate the name.
- No `mod.rs`: modules with children are `foo.rs` plus `foo/`. Integration tests live under
  `tests/it/`.
- No separator comments (`// -----`, `// =====`). If a file needs sections, split it into
  submodules.
- Leave a blank line after the closing `}` of a block statement when another statement
  follows, and between logical phases of a function. No lumped code.
- One level of abstraction per function. Long loops (simulations, game ticks, solvers) are
  short drivers that call documented step functions. If a block needs a "what" comment,
  extract it into a function named after that comment.
- `clippy::pedantic` clean. Suppression only with a scoped
  `#[expect(lint, reason = "...")]`. Never `#[allow]`, and never crate-level suppression.
- No `unsafe` unless the crate opts out of `#![forbid(unsafe_code)]` with a documented
  reason. Each unsafe block has one operation and a `// SAFETY:` comment.
- The crate is `#![no_std]` without `alloc` and has no runtime dependencies (the CI `no-std`
  job). Flag any new `[dependencies]` entry, any `std` or `alloc` use, and any feature-gated
  item without its `#[cfg]` on the re-export in `lib.rs`.
- `Valuation::marginal_score` scores adding a task to an agent's current bundle: higher is
  better, and `None` means the agent cannot take the task, so it places no bid.
- Public API changes go with a matching update to the README scope list and usage example
  and the crate docs in `lib.rs`. Name every public API change in the PR.
- Mermaid diagrams in doc comments must match the code they describe. Update them when the
  structure changes.
