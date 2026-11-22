---
applyTo: ".github/workflows/**,Taskfile.dist.yaml,**/Taskfile.dist.yaml,.pre-commit-config.yaml"
---

# CI, Taskfile and hooks stay in sync

- The `ci` task in `Taskfile.dist.yaml` mirrors the jobs in `.github/workflows/ci.yml`. A
  change that adds, removes or renames a CI job, or changes its flags, must make the same
  change to `task ci` and the task it calls, and the other way round. Flag any drift in review.
- Local hooks in `.pre-commit-config.yaml` call Taskfile tasks. They must not duplicate
  commands.
- Every `uses:` is pinned to a full 40-character commit SHA, with a `# vX.Y.Z` comment. Reject
  tag or branch refs.
- `dtolnay/rust-toolchain` is pinned to a `master` commit (`# master`) and always gets an
  explicit `toolchain:` input. Its `stable`/`nightly` branches are not tags, and Dependabot
  cannot follow them: it re-pinned the `nightly` use to the `stable` commit.
- The workflow sets `permissions: {}` at the top, and each job sets the least privilege it
  needs. Every `actions/checkout` sets `persist-credentials: false`.
- No `pull_request_target`. No `${{ github.event.* }}` or other untrusted expressions
  directly in `run:`. Pass them through `env:`.
- Do not rename `publish-crates-io.yml` or its `crates-io` environment. crates.io trusted publishing is
  bound to both. Deploy jobs run in their own tag-restricted environment, and `needs:` a
  verification job.
- CI jobs: `branch`, `fmt`, `clippy`, `style`, `doc`, `test` (Linux only), `coverage`,
  `deny`, `msrv`, `semver`, `no-std`, plus `actionlint` and `zizmor` in `actions-lint.yml`.
- `no-std` mirrors `task build:no-std`: the library for `thumbv7em-none-eabihf`. Do not drop it.
- `msrv` checks the library only (`cargo hack check --rust-version --all-features`, no
  `--all-targets`): dev-dependencies may need a newer Rust. Do not add `--all-targets` back.
- No `Cargo.lock` is tracked: this is a library. CI resolves fresh from crates.io, and no
  step uses `--locked`.
- `dependencies` (`dependency submission`) in `ci.yml` runs only on pushes to `main`: it
  generates an SPDX SBOM with cargo-sbom and submits it to the dependency graph. It is
  CI-only and not a required check.
- `zizmor-sarif` (`zizmor (code scanning)`) in `actions-lint.yml` runs only on pushes to
  `main` and uploads zizmor's findings as SARIF to the Security tab without failing. The
  blocking `zizmor` job stays the merge gate; `zizmor-sarif` is not a required check.
- Both jobs are skipped while the repository is private.
- `scripts/check-branch.sh` is the single definition of allowed branch names. It is used by
  the Taskfile, the hook and the CI `branch` job.
- New jobs that should gate merges must also be added to branch protection. Mention that in
  the PR.
