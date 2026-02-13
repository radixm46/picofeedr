PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS es_meta (
    id INTEGER PRIMARY KEY,
    meta_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_key TEXT NOT NULL UNIQUE,
    url TEXT NOT NULL,
    title TEXT,
    author TEXT,
    site_url TEXT,
    meta_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER
);

CREATE TABLE IF NOT EXISTS entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_key TEXT NOT NULL UNIQUE,
    feed_id INTEGER NOT NULL,
    source_id TEXT,
    link TEXT,
    title TEXT,
    author TEXT,
    published_at INTEGER,
    updated_at INTEGER,
    first_seen_at INTEGER NOT NULL,
    meta_json TEXT,
    FOREIGN KEY(feed_id) REFERENCES feeds(id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS entry_enclosures (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    mime_type TEXT,
    length INTEGER,
    FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS entry_contents (
    entry_id INTEGER PRIMARY KEY,
    storage TEXT NOT NULL,
    ref TEXT,
    content_type TEXT,
    content TEXT,
    FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS entry_tags (
    entry_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (entry_id, tag_id),
    FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE,
    FOREIGN KEY(tag_id) REFERENCES tags(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_feeds_feed_key ON feeds(feed_key);
CREATE INDEX IF NOT EXISTS idx_feeds_url ON feeds(url);

CREATE INDEX IF NOT EXISTS idx_entries_entry_key ON entries(entry_key);
CREATE INDEX IF NOT EXISTS idx_entries_feed_id ON entries(feed_id);
CREATE INDEX IF NOT EXISTS idx_entries_feed_published ON entries(feed_id, published_at);
CREATE INDEX IF NOT EXISTS idx_entries_feed_first_seen ON entries(feed_id, first_seen_at);
CREATE INDEX IF NOT EXISTS idx_entries_published ON entries(published_at);
CREATE INDEX IF NOT EXISTS idx_entries_first_seen ON entries(first_seen_at);
CREATE INDEX IF NOT EXISTS idx_entries_feed_source ON entries(feed_id, source_id);
CREATE INDEX IF NOT EXISTS idx_entries_link ON entries(link);
CREATE INDEX IF NOT EXISTS idx_entries_effective_date ON entries(COALESCE(published_at, updated_at, first_seen_at), id);
CREATE INDEX IF NOT EXISTS idx_entries_feed_effective_date ON entries(feed_id, COALESCE(published_at, updated_at, first_seen_at), id);

CREATE INDEX IF NOT EXISTS idx_entry_enclosures_entry ON entry_enclosures(entry_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_entry_enclosures_entry_url ON entry_enclosures(entry_id, url);

CREATE INDEX IF NOT EXISTS idx_entry_contents_ref ON entry_contents(ref);

CREATE INDEX IF NOT EXISTS idx_entry_tags_tag_entry ON entry_tags(tag_id, entry_id);
CREATE INDEX IF NOT EXISTS idx_entry_tags_entry ON entry_tags(entry_id);
CREATE INDEX IF NOT EXISTS idx_entry_tags_tag ON entry_tags(tag_id);

CREATE INDEX IF NOT EXISTS idx_tags_name ON tags(name);
