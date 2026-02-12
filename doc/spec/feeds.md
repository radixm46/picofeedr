# Feed Source Spec (`feeds.yaml`)

This document defines the current, implementation-aligned behavior of feed source loading.
It intentionally captures current contracts before introducing behavior changes.

## Scope

- Input file format for `feeds.yaml`
- How feed entries are discovered and flattened
- Which keys are interpreted and which are ignored
- Validation semantics used by `feeds --config-check`

## Top-Level Contract

- `feeds` is required.
- `feeds` must be a YAML mapping.
- `auto_tags` is optional and, when present, is parsed as auto-tag rules.
- Top-level keys other than `feeds` and `auto_tags` are currently ignored by the loader.

If top-level `feeds` is missing, loading fails with a configuration error.

## Feed Tree Model

The value under top-level `feeds` is a nested group tree.

Each group is a YAML mapping. Within a group:

- `tags`: optional list of strings inherited by descendants
- `feeds`: optional list of feed entries
- any other key: treated as a nested child group

This means unknown keys inside a group are not ignored; they are interpreted as subgroup nodes.

## Feed Entry Contract

Each item in a group `feeds` list must be a mapping with:

- `url` (required, string)
- `title` (optional, string)
- `tags` (optional, list of strings)

The loader trims surrounding whitespace from `url`.

## Flattening and Tag Inheritance

- The tree is flattened into a linear feed list.
- Group tags are inherited downward.
- Feed-level tags are appended after inherited tags.
- Tag order is preserved by first appearance.
- Duplicate tags are de-duplicated while keeping first-seen order.

## Auto Tags

- Top-level `auto_tags` is parsed independently from the feed tree.
- `auto_tags` participates in validation and sync-time rule compilation.
- Auto-tag application itself occurs during sync for newly ingested entries.

## Validation (`feeds --config-check`)

The static validator checks:

- empty feed URL -> error (`EMPTY_FEED_URL`)
- duplicated feed URL -> error (`DUPLICATE_FEED_URL`)
- invalid auto-tag rule shape -> error (`INVALID_AUTO_TAG_RULE`)
- duplicated tags within a feed entry -> warning (`DUPLICATE_FEED_TAG`)

Validation reports include logical YAML paths for issue locations.

## Sync-Relevant Behavior

- Sync targets are built from the flattened `feeds` list only.
- URLs removed from `feeds.yaml` are not fetched, while historical DB rows may remain.

## Non-Goals of This Document

- This document does not define a future behavior change yet.
- In particular, it does not redefine `auto_tags` handling or unknown-key policy beyond current behavior.
