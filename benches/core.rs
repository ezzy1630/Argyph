#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn bench_walk_directory(c: &mut Criterion) {
    let root = camino::Utf8PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    c.bench_function("walk_project_root", |b| {
        b.iter(|| {
            let count = walkdir::WalkDir::new(root.as_str())
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .count();
            black_box(count)
        });
    });
}

fn bench_token_count(c: &mut Criterion) {
    let src = include_str!("../examples/tiny-rust-app/src/main.rs");
    c.bench_function("token_count_rust_file", |b| {
        let bpe = tiktoken_rs::cl100k_base().unwrap();
        b.iter(|| {
            let tokens = bpe.encode_ordinary(src);
            black_box(tokens.len())
        });
    });
}

criterion_group!(benches, bench_walk_directory, bench_token_count);
criterion_main!(benches);
