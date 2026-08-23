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

    let tag_names = [
        "unread", "rare", "news", "later", "junk", "youtube", "github", "alpha", "beta", "gamma",
        "delta", "epsilon", "zeta", "eta",
    ];
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
    for idx in 0..entry_count {
        let entry_pk = (idx + 1) as i64;
        let ts = base_ts + idx as i64 * 60;
        let entry_id = format!("entry-{idx}");
        let link = format!("https://example.com/{idx}");
        let title = format!("Entry {idx}");

        insert_entry
            .execute(params![entry_pk, entry_id, link, title, ts])
            .expect("insert entry");

        // Every entry is unread; rare is a 1% posting list for the direct-count path.
        insert_entry_tag
            .execute(params![entry_pk, 1i64])
            .expect("insert unread tag");
        if idx % 100 == 0 {
            insert_entry_tag
                .execute(params![entry_pk, 2i64])
                .expect("insert excluded tag");
        }

        // These real posting lists are used by the negative complex benchmark.
        let broad_tag_id = 3 + (idx % 4) as i64;
        insert_entry_tag
            .execute(params![entry_pk, broad_tag_id])
            .expect("insert broad tag");
        if idx % 5 == 0 {
            insert_entry_tag
                .execute(params![entry_pk, 7i64])
                .expect("insert github tag");
        }

        // Each seven-way OR operand has a real posting list.
        let branch_tag_id = 8 + (idx % 7) as i64;
        insert_entry_tag
            .execute(params![entry_pk, branch_tag_id])
            .expect("insert branch tag");
    }

    drop(insert_entry_tag);
    drop(insert_entry);
    tx.commit().expect("commit fixture");

    BenchFixture {
        _temp: temp,
        db_path,
    }
}

fn bench_tag_queries(c: &mut Criterion) {
    let fixture = build_fixture(100_000);
    let store = SqliteStore::open(&fixture.db_path).expect("open query store");

    let simple_cases = [
        ("single_tag_broad", "tag:unread", 100_000),
        ("single_tag_rare", "tag:rare", 1_000),
        ("single_tag_zero", "tag:doesnotexist", 0),
        ("single_tag_with_title", "tag:unread Entry 99999", 1),
    ];
    let mut simple_group = c.benchmark_group("tag_query_simple");
    for (name, raw_query, expected_total) in simple_cases {
        let query = EntryQuery::parse(Some(raw_query), Some("unread")).expect("parse query");
        let baseline =
            list_entries(&store, &query, SortOrder::DateDesc, 100, None).expect("baseline query");
        assert_eq!(baseline.total_count, expected_total);
        simple_group.bench_with_input(BenchmarkId::from_parameter(name), &query, |b, query| {
            b.iter(|| {
                let result = list_entries(&store, query, SortOrder::DateDesc, 100, None)
                    .expect("query result");
                criterion::black_box(result.total_count)
            });
        });
    }
    simple_group.finish();

    let complex_cases = [
        ("negative_real_tags", "tag:unread -tag:news", 75_000),
        (
            "seven_real_tags_or",
            "tag:alpha|beta|gamma|delta|epsilon|zeta|eta",
            100_000,
        ),
        (
            "nested_real_tags",
            "tag:(unread&(alpha|beta)) -tag:delta",
            28_572,
        ),
    ];
    let mut complex_group = c.benchmark_group("tag_query_complex");
    for (name, raw_query, expected_total) in complex_cases {
        let query = EntryQuery::parse(Some(raw_query), Some("unread")).expect("parse query");
        let baseline =
            list_entries(&store, &query, SortOrder::DateDesc, 100, None).expect("baseline query");
        assert_eq!(baseline.total_count, expected_total);
        complex_group.bench_with_input(BenchmarkId::from_parameter(name), &query, |b, query| {
            b.iter(|| {
                let result = list_entries(&store, query, SortOrder::DateDesc, 100, None)
                    .expect("query result");
                criterion::black_box(result.total_count)
            });
        });
    }
    complex_group.finish();

    let mut scaling_group = c.benchmark_group("tag_query_size_scaling");
    for entry_count in [10_000, 50_000, 100_000] {
        let fixture = build_fixture(entry_count);
        let store = SqliteStore::open(&fixture.db_path).expect("open query store");
        let query = EntryQuery::parse(Some("tag:unread"), Some("unread")).expect("parse query");
        let baseline =
            list_entries(&store, &query, SortOrder::DateDesc, 100, None).expect("baseline query");
        assert_eq!(baseline.total_count, entry_count as i64);
        scaling_group.bench_with_input(
            BenchmarkId::from_parameter(entry_count),
            &query,
            |b, query| {
                b.iter(|| {
                    let result = list_entries(&store, query, SortOrder::DateDesc, 100, None)
                        .expect("query result");
                    criterion::black_box(result.total_count)
                });
            },
        );
    }
    scaling_group.finish();
}

criterion_group!(benches, bench_tag_queries);
criterion_main!(benches);
