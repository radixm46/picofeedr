# Issue #69 Complex Path Performance Recovery (hyperfine)

## Protocol
- compared commits:
  - pre: `9968469`
  - post: `codex/issue-69-complex-path-perf-recovery` HEAD
- dataset:
  - 100,000 entries
  - all entries tagged `unread`
  - first 25,000 entries additionally tagged with one of `news/later/junk/YouTube` (round-robin)
- command options:
  - `--warmup 3 --runs 30 --export-json`

## Commands
- complex:
  - `--output json list --sort date-asc --query 'tag:unread -tag:news|later|junk|YouTube'`
- simple:
  - `--output json list --query unread`

## Result (mean seconds)
- complex:
  - pre: `0.2599`
  - post: `0.1573` (`~39.5%` faster)
- simple:
  - pre: `0.0725`
  - post: `0.0774` (`~6.7%` slower, below 15% guardrail)

## Artifacts
- `complex_pre_post_v3.json`
- `simple_pre_post_v3.json`
