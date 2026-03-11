# Feed Source Spec (`feeds.yaml`)

## Scope

This document defines the current contract for loading `feeds.yaml`.

It covers:

- accepted top-level structure
- feed tree flattening
- tag inheritance
- auto-tag rule placement
- validation semantics used by `feeds --config-check`

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

The loader trims surrounding whitespace from `url`.

## Flattening Contract

- the group tree is flattened into a linear feed list
- group tags are inherited downward
- feed-level tags are appended after inherited tags
- tag order is preserved by first appearance
- duplicate tags are removed while keeping first-seen order

## Auto Tags

- `auto_tags` may be defined at the root and at any nested group
- parent `auto_tags` are inherited by descendant groups and feeds
- effective feed rules are the merged list of inherited + local rules
- `title_contains` matching is case-insensitive
- `priority` is optional and defaults to `0`
- auto-tag application occurs during sync for newly ingested entries only

## Validation Contract

The static validator checks at least the following:

- empty feed URL -> error (`EMPTY_FEED_URL`)
- duplicated feed URL -> error (`DUPLICATE_FEED_URL`)
- invalid auto-tag rule shape -> error (`INVALID_AUTO_TAG_RULE`)
- duplicated tags within a feed entry -> warning (`DUPLICATE_FEED_TAG`)

Validation reports include logical YAML paths for issue locations.

## Sync-Relevant Behavior

- sync targets are built from the flattened `feeds` list only
- URLs removed from `feeds.yaml` are not fetched
- historical DB rows may remain after a feed URL is removed from `feeds.yaml`

## Non-Goals

- this document does not define a strict mode such as `--strict`
- this document does not define CLI output shapes beyond the validation payload
