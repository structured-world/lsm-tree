# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Fork:** [structured-world/coordinode-lsm-tree](https://github.com/structured-world/coordinode-lsm-tree),
> a maintained fork of [fjall-rs/lsm-tree](https://github.com/fjall-rs/lsm-tree).
> Fork releases use `v`-prefixed tags (`v4.0.0`); upstream uses bare tags (`3.1.2`).

## [Unreleased]

## [5.0.0](https://github.com/structured-world/coordinode-lsm-tree/compare/v4.0.0...v5.0.0) - 2026-03-23

### Added

- *(compression)* zstd dictionary compression support ([#131](https://github.com/structured-world/coordinode-lsm-tree/pull/131))

### Fixed

- thread UserComparator through ingestion guards and range overlap ([#139](https://github.com/structured-world/coordinode-lsm-tree/pull/139))
