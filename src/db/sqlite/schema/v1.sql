PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS es_meta (
    id INTEGER PRIMARY KEY,
    meta_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id TEXT NOT NULL UNIQUE CHECK(feed_id <> ''),
    url TEXT NOT NULL CHECK(url <> ''),
    title TEXT,
    author TEXT,
    site_url TEXT,
    meta_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER
);

CREATE TABLE IF NOT EXISTS entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id TEXT NOT NULL UNIQUE CHECK(entry_id <> ''),
    feed_pk INTEGER NOT NULL,
    link TEXT,
    title TEXT,
    author TEXT,
    published_at INTEGER,
    updated_at INTEGER,
    first_seen_at INTEGER NOT NULL,
    meta_json TEXT,
    FOREIGN KEY(feed_pk) REFERENCES feeds(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS entry_enclosures (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_pk INTEGER NOT NULL,
    url TEXT NOT NULL,
    mime_type TEXT,
    length INTEGER,
    FOREIGN KEY(entry_pk) REFERENCES entries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS entry_contents (
    entry_pk INTEGER PRIMARY KEY,
    storage TEXT NOT NULL CHECK(storage IN ('db', 'fs', 'none')),
    ref TEXT,
    content_type TEXT,
    content TEXT,
    FOREIGN KEY(entry_pk) REFERENCES entries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE CHECK(name <> '')
);

CREATE TABLE IF NOT EXISTS entry_tags (
    entry_pk INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (entry_pk, tag_id),
    FOREIGN KEY(entry_pk) REFERENCES entries(id) ON DELETE CASCADE,
    FOREIGN KEY(tag_id) REFERENCES tags(id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_entries_feed_published ON entries(feed_pk, published_at);
CREATE INDEX IF NOT EXISTS idx_entries_feed_first_seen ON entries(feed_pk, first_seen_at);
CREATE INDEX IF NOT EXISTS idx_entries_published ON entries(published_at);
CREATE INDEX IF NOT EXISTS idx_entries_first_seen ON entries(first_seen_at);
CREATE INDEX IF NOT EXISTS idx_entries_effective_date ON entries(COALESCE(published_at, updated_at, first_seen_at), id);
CREATE INDEX IF NOT EXISTS idx_entries_feed_effective_date ON entries(feed_pk, COALESCE(published_at, updated_at, first_seen_at), id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_entry_enclosures_entry_pk_url ON entry_enclosures(entry_pk, url);

CREATE INDEX IF NOT EXISTS idx_entry_contents_ref ON entry_contents(ref);

CREATE INDEX IF NOT EXISTS idx_entry_tags_tag_entry ON entry_tags(tag_id, entry_pk);
