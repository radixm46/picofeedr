use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use picofeedr::cli::SortOrder;
use picofeedr::db::sqlite::SqliteStore;
use picofeedr::entry::list_entries;
use picofeedr::query::EntryQuery;
use rusqlite::{Connection, params};
use tempfile::TempDir;

struct BenchFixture {
    _temp: TempDir,
    db_path: std::path::PathBuf,
    query: EntryQuery,
    expected_total: i64,
}

fn build_fixture(entry_count: usize) -> BenchFixture {
    let temp = TempDir::new().expect("tempdir");
    let db_path = temp.path().join("bench.sqlite");

    {
        let store = SqliteStore::open(&db_path).expect("open sqlite");
        store.migrate().expect("migrate schema");
    }

    let mut conn = Connection::open(&db_path).expect("open sqlite connection");
    let tx = conn.transaction().expect("begin tx");

    tx.execute(
        "INSERT INTO feeds (id, feed_id, url, title, author, site_url, meta_json, created_at, updated_at) \
         VALUES (1, 'bench-feed', 'https://example.com/feed.xml', 'Bench Feed', NULL, NULL, NULL, 1704067200, 1704067200)",
        [],
    )
    .expect("insert feed");

    let tag_names = ["unread", "news", "later", "junk", "youtube", "github"];
    for name in tag_names {
        tx.execute("INSERT INTO tags (name) VALUES (?1)", params![name])
            .expect("insert tag");
    }

    let mut insert_entry = tx
        .prepare(
            "INSERT INTO entries (id, entry_id, feed_pk, link, title, author, published_at, updated_at, first_seen_at, meta_json) \
             VALUES (?1, ?2, 1, ?3, ?4, NULL, ?5, ?5, ?5, NULL)",
        )
        .expect("prepare insert entry");
    let mut insert_entry_tag = tx
        .prepare("INSERT INTO entry_tags (entry_pk, tag_id) VALUES (?1, ?2)")
        .expect("prepare insert entry_tag");

    let base_ts = 1_704_067_200i64;
    let mut expected_total = 0i64;
    for idx in 0..entry_count {
        let entry_pk = (idx + 1) as i64;
        let ts = base_ts + idx as i64 * 60;
        let entry_id = format!("entry-{idx}");
        let link = format!("https://example.com/{idx}");
        let title = format!("Entry {idx}");

        insert_entry
            .execute(params![entry_pk, entry_id, link, title, ts])
            .expect("insert entry row");

        // unread
        insert_entry_tag
            .execute(params![entry_pk, 1i64])
            .expect("insert unread tag");

        // Apply one exclusion tag to 25% of entries.
        if idx % 4 == 0 {
            let excluded_tag_id = 2 + ((idx / 4) % 5) as i64;
            insert_entry_tag
                .execute(params![entry_pk, excluded_tag_id])
                .expect("insert excluded tag");
        } else {
            expected_total += 1;
        }
    }

    drop(insert_entry_tag);
    drop(insert_entry);
    tx.commit().expect("commit fixture");

    let query = EntryQuery::parse(
        Some("tag:unread -tag:news|later|junk|youtube|github after:2024-01-01 before:2025-01-01"),
        Some("unread"),
    )
    .expect("parse query");

    BenchFixture {
        _temp: temp,
        db_path,
        query,
        expected_total,
    }
}

fn bench_complex_tag_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("tag_query_complex");
    for &size in &[10_000usize, 50_000usize, 100_000usize] {
        let fixture = build_fixture(size);
        let store = SqliteStore::open(&fixture.db_path).expect("open query store");

        let baseline = list_entries(&store, &fixture.query, SortOrder::DateDesc, 100, None)
            .expect("baseline query");
        assert_eq!(baseline.total_count, fixture.expected_total);

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let result = list_entries(&store, &fixture.query, SortOrder::DateDesc, 100, None)
                    .expect("query result");
                criterion::black_box(result.total_count)
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_complex_tag_query);
criterion_main!(benches);
