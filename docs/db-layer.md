# DB Layer Responsibilities

This document defines strict boundaries for the SQLite persistence layer.

## `schema/`
- Owns DDL and migration assets.
- Contains SQL for schema initialization and migration only.

## `query/`
- Owns SQL text constants and SQL builder helpers.
- Does not execute SQL.
- Does not contain business logic.

## DAO (`entries.rs`, `feeds.rs`, `tags.rs`, `meta.rs`)
- Owns single-statement SQL execution.
- Must not implement use-case orchestration.
- Must stay `pub(crate)` and internal to `db::sqlite`.

## `repo/`
- Owns use-case orchestration over multiple DAO calls.
- Read paths live in `*ReadRepo`.
- Write paths live in `*WriteRepo` and are expected to run under `Tx`.

## Transaction usage
- New write flows must use `SqliteStore::tx()` and `*WriteRepo`.
- `SqliteStore::transaction()` is compatibility-only and should not be used in new code.
