# Changelog

All notable changes to `fixtures` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.7.0] - 2026-04-05

### Features

- feat: add default_branch support and head branch detection fixtures (#29)

## [0.6.0] - 2026-04-04

### Features

- feat: ignore unknown fields in definitions and disable skipCi (#27)

## [0.5.0] - 2026-04-04

### Features

- feat: make config field optional for tool-agnostic fixture generation (#26)

## [0.4.0] - 2026-04-04

### Features

- feat: migrate fixture definitions from TOML to JSON (#24)

## [0.3.0] - 2026-04-04

### Features

- feat: add bulk generation mode for benchmarks and stress tests (#20)

## [0.2.3] - 2026-04-04

## [0.2.2] - 2026-04-04

### Bug Fixes

- fix: resolve ferrflow binary to absolute path before cd into fixtures (#18)

## [0.2.1] - 2026-04-04

### Bug Fixes

- fix: add workspace marker to prevent parent workspace detection (#17)

## [0.2.0] - 2026-04-04

### Features

- feat: reusable GitHub Action for fixture generation and testing (#14)
- feat: extend generator with tags, hooks, config formats, and merge commits (#13)
- feat: initial test infrastructure with fixture generator, runner, and 9 scenarios

### Bug Fixes

- fix(ci): use FERRFLOW_TOKEN for release push authentication (#15)
