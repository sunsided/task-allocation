# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.0](https://github.com/sunsided/task-allocation/releases/tag/v0.0.0) - 2026-11-22

### Added

- Add bundle replanning, so CBBA can free blocked slots
- Add examples that pair allocation with the sibling planning crates
- Add timed routes as a ready-made path score ([#9](https://github.com/sunsided/task-allocation/pull/9))
- Add sequential and combinatorial auction baselines ([#8](https://github.com/sunsided/task-allocation/pull/8))
- Add timed routes as a ready-made path score
- Add sequential and combinatorial auction baselines
- Add network topologies, so tests cover relayed messages
- Add CBBA with a deterministic simulated network
- Add greedy bundle construction and centralized references
- Add dormant release automation with release-plz
- Add CONTRIBUTING.md
- Add git hooks and Copilot review instructions
- Add README badges, a usage example and package excludes
- Add REUSE licensing metadata
- Add supply-chain checks
- Add Codecov configuration
- Add Taskfile and CI workflows

### Changed

- Test that simulations can be cloned
- Let the simulation run any consensus protocol
- Describe full replanning's outcome and cost precisely
- Keep the examples' gains diminishing, and check it
- Attribute the greedy claim to the literature, and trace the counterexample
- Round halving rewards down, as documented
- Auction each task once and accept any task identifier
- Require a finished bundle phase before a team counts as settled
- Test the reset action through an agent
- Cover message accessors and empty teams
- Bound the exhaustive optimum to ten tasks
- Separate the README example's setup from its checks
- Test the Valuation trait through a minimal deadline rover
- Tighten lints and forbid unsafe code
- Render mermaid diagrams on docs.rs and doctest the README
- Reserve the crate name with a 0.0.0 placeholder

### Dependencies

- Bump taiki-e/install-action to v2.90.8

### Fixed

- Correct the stated greedy approximation guarantees
