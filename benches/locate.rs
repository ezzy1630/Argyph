#![allow(clippy::unwrap_used, clippy::expect_used)]

use argyph_locate::path;
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn bench_path_parse(c: &mut Criterion) {
    let heading = "docs/billing.md > Enterprise";
    c.bench_function("locate_parse_path_heading", |b| {
        b.iter(|| {
            black_box(path::parse(heading));
        });
    });

    let bare = "Cargo.toml";
    c.bench_function("locate_parse_path_bare", |b| {
        b.iter(|| {
            black_box(path::parse(bare));
        });
    });

    let wildcard = "**/*.json > database";
    c.bench_function("locate_parse_path_wildcard", |b| {
        b.iter(|| {
            black_box(path::parse(wildcard));
        });
    });
}

fn bench_strategy_dispatch(c: &mut Criterion) {
    use argyph_locate::strategy;

    c.bench_function("locate_strategy_path_only", |b| {
        b.iter(|| {
            black_box(strategy::plan(
                None,
                Some("Cargo.toml > package.name"),
                true,
            ));
        });
    });

    c.bench_function("locate_strategy_query_short", |b| {
        b.iter(|| {
            black_box(strategy::plan(Some("billing"), None, true));
        });
    });

    c.bench_function("locate_strategy_scoped", |b| {
        b.iter(|| {
            black_box(strategy::plan(
                Some("enterprise pricing"),
                Some("docs/billing.md"),
                true,
            ));
        });
    });
}

criterion_group!(benches, bench_path_parse, bench_strategy_dispatch);
criterion_main!(benches);
