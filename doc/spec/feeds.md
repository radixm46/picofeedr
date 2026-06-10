# Feed Source Spec (`feeds.yaml`)

## Scope

This document defines the current contract for loading `feeds.yaml`.

It covers:

- accepted top-level structure
- feed tree flattening
- tag inheritance
- auto-tag rule placement
- validation semantics used by `sync --check` and `sync`

## Role Separation

`feeds.yaml` is the source for feed rules and the sync catalog:

- feed URLs that `sync` may fetch
- configured feed titles used when sync registers feed rows
- feed-to-tag rules inherited from groups
- auto-tag rules
- skipped feed declarations

The local database is the source for persisted state:

- known feed rows and last observed feed metadata
- the tag dictionary
- entry-to-tag relations
- sync/write status metadata

The `feeds` CLI command reads feed state from the database only. It does not load
or validate `feeds.yaml`, register configured feeds, or create tags.

## Top-Level Contract

- `picofeedr` is required.
- `picofeedr` must be a YAML mapping.
- top-level keys other than `picofeedr` are ignored.

If top-level `picofeedr` is missing, loading fails with a configuration error.

## Group Model

The value under `picofeedr` is a nested group tree.

Each group may define:

- `tags`: optional list of strings inherited by descendants
- `auto_tags`: optional list of auto-tag rules inherited by descendants
- `feeds`: optional list of feed entries
- any other key: treated as a nested child group

Arbitrary group names are supported.

## Feed Entry Contract

Each item in a group `feeds` list must be a mapping with:

- `url` (required, string)
- `title` (optional, string)
- `tags` (optional, list of strings)
- `skip` (optional, boolean; defaults to `false`)

The loader trims surrounding whitespace from `url`.

`skip: true` keeps the feed entry in `feeds.yaml` but excludes it from sync fetch
targets. Skipped feed entries are still loaded and validated by `sync --check`.

Currently supported feed source URL schemes are:

- `http://`
- `https://`
- `gopher://`
- `file://`

## Flattening Contract

- the group tree is flattened into a linear feed list
- group tags are inherited downward
- feed-level tags are appended after inherited tags
- tag order is preserved by first appearance
- duplicate tags are removed while keeping first-seen order

## Tag Layers

Tags are described at three layers:

- L1 dictionary: rows in the `tags` table
- L2 feed-to-tag rules: inherited and feed-level `tags` declared in `feeds.yaml`
- L3 entry-to-tag facts: rows in `entry_tags`

L2 declarations do not create L1 dictionary rows by themselves. L1 rows are
created when a command writes actual tag facts, such as sync ingest creating L3
entry tags or `mark tag --add` adding tags to existing entries.

## Auto Tags

- `auto_tags` may be defined at the root and at any nested group
- parent `auto_tags` are inherited by descendant groups and feeds
- effective feed rules are the merged list of inherited + local rules
- `title_contains` matching is case-insensitive
- `priority` is optional and defaults to `0`
- auto-tag application occurs during sync for newly ingested entries only

## Validation Contract

The shared validator checks at least the following:

- empty feed URL -> error (`EMPTY_FEED_URL`)
- duplicated feed URL -> error (`DUPLICATE_FEED_URL`)
- invalid auto-tag rule shape -> error (`INVALID_AUTO_TAG_RULE`)
- invalid `title_regex` pattern -> error (`INVALID_TITLE_REGEX`)
- duplicated tags within a feed entry -> warning (`DUPLICATE_FEED_TAG`)

Feeds with `skip: true` remain part of validation and are counted in `checked_feeds`.
The validation report also includes `skipped_feeds`.

Validation reports include logical YAML paths for issue locations.

Blocking validation errors make `sync` fail with a configuration error before it
starts fetching feeds.

`sync --check` returns the same validation result as a report payload and exits with code 1
when blocking errors are present.

## Sync-Relevant Behavior

- sync targets are built from the flattened `feeds` list only
- feed entries with `skip: true` are not fetched or ingested during sync
- `http://`, `https://`, `gopher://`, and `file://` source URLs are fetchable
- `gopher://` sources are fetched as raw feed documents and parsed by the same RSS/Atom parser used for HTTP/file sources
- URLs removed from `feeds.yaml` are not fetched
- historical DB rows may remain after a feed URL is removed from `feeds.yaml`

## Non-Goals

- this document does not define a strict mode such as `--strict`
- this document does not define CLI output shapes beyond the validation payload
- this document does not define database cleanup for removed feeds or orphan tags;
  cleanup belongs to an independent maintenance command
