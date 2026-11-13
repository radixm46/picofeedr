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
                std::hint::black_box(result.total_count)
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
                std::hint::black_box(result.total_count)
            });
        });
    }
    complex_group.finish();

    let query = EntryQuery::parse(Some("tag:unread -tag:news"), Some("unread"))
        .expect("parse pagination query");
    let deep_cursor = {
        let page = list_entries(&store, &query, SortOrder::DateDesc, 50_000, None)
            .expect("prepare deep cursor");
        assert_eq!(page.total_count, 75_000);
        assert_eq!(page.items.len(), 50_000);
        page.next_page_token.expect("deep cursor")
    };
    let mut pagination_group = c.benchmark_group("tag_query_complex_pagination");
    for (name, cursor, offset) in [
        ("first", None, 0),
        ("after_50000", Some(deep_cursor.as_str()), 50_000),
    ] {
        let baseline =
            list_entries(&store, &query, SortOrder::DateDesc, 100, cursor).expect("baseline page");
        let expected_ids = (0..100_000)
            .rev()
            .filter(|idx| idx % 4 != 0)
            .skip(offset)
            .take(100)
            .map(|idx| format!("entry-{idx}"))
            .collect::<Vec<_>>();
        let actual_ids = baseline
            .items
            .iter()
            .map(|item| item.entry_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(baseline.total_count, 75_000);
        assert_eq!(actual_ids, expected_ids, "{name}");
        assert!(baseline.next_page_token.is_some());
        pagination_group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &cursor,
            |b, cursor| {
                b.iter(|| {
                    let result = list_entries(&store, &query, SortOrder::DateDesc, 100, *cursor)
                        .expect("page result");
                    std::hint::black_box(result)
                });
            },
        );
    }
    pagination_group.finish();

    let mut scaling_group = c.benchmark_group("tag_query_size_scaling");
    for entry_count in [10_000, 50_000, 100_000] {
        let fixture = build_fixture(entry_count);
        let store = SqliteStore::open(&fixture.db_path).expect("open query store");
        for (name, raw_query, expected_total) in [
            ("single_tag_broad", "tag:unread", entry_count),
            (
                "negative_real_tags",
                "tag:unread -tag:news",
                entry_count * 3 / 4,
            ),
        ] {
            let query = EntryQuery::parse(Some(raw_query), Some("unread")).expect("parse query");
            let baseline = list_entries(&store, &query, SortOrder::DateDesc, 100, None)
                .expect("baseline query");
            assert_eq!(baseline.total_count, expected_total as i64);
            scaling_group.bench_with_input(
                BenchmarkId::new(name, entry_count),
                &query,
                |b, query| {
                    b.iter(|| {
                        let result = list_entries(&store, query, SortOrder::DateDesc, 100, None)
                            .expect("query result");
                        std::hint::black_box(result.total_count)
                    });
                },
            );
        }
    }
    scaling_group.finish();
}

criterion_group!(benches, bench_tag_queries);
criterion_main!(benches);
