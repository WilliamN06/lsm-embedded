# lsm-embedded

[![Crates.io](https://img.shields.io/crates/v/lsm-embedded)](https://crates.io/crates/lsm-embedded)
[![Docs](https://docs.rs/lsm-embedded/badge.svg)](https://docs.rs/lsm-embedded)
[![CI](https://github.com/yourusername/lsm-embedded/actions/workflows/ci.yml/badge.svg)](https://github.com/yourusername/lsm-embedded/actions)
[![License: MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

**Write-optimised LSM storage for WASM and embedded devices.**

[Features](#features) •
[Quick Start](#quick-start) •
[Documentation](#documentation) •
[Benchmarks](#benchmarks) •
[Contributing](#contributing)

---

## The Problem

Edge devices need fast write-optimised storage, but existing solutions don't fit:

- **SQLite** → Slow writes on flash, B-tree fragmentation
- **RocksDB** → Won't compile to WASM, 100MB+ memory
- **LMDB** → Memory-maps the entire database
- **Sled** → No `no_std` support, no WASM

**Our solution:** A lightweight LSM-tree storage engine designed specifically for constrained environments.

---

## Features

-  **`no_std` support** — Works on embedded devices without a heap
-  **WASM support** — Compiles to WebAssembly for edge computing
-  **Fixed memory footprint** — No dynamic allocation in the critical path
-  **Write-optimised** — Tiered compaction for high ingestion rates
-  **Small footprint** — ~8MB peak memory usage
-  **Flexible storage** — Works with files, SPI flash, or in-memory
-  **Rust safe code** — `#![forbid(unsafe_code)]`

---

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
lsm-embedded = "0.1.0"