# Argyph Benchmarks

This directory contains the [criterion](https://github.com/bheisler/criterion.rs)
benchmark harness used to track the performance of Argyph's hot paths.

The canonical results (with hardware tag, methodology, and acceptance
thresholds) live in [`../docs/benchmarks.md`](../docs/benchmarks.md).
That file is the source of truth for any number quoted in the README.

## Running locally

```bash
# Whole-workspace benchmark sweep.
cargo bench --workspace

# Single bench file.
cargo bench --bench locate

# A single named benchmark.
cargo bench --bench locate -- locate_strategy_scoped
```

Criterion writes its HTML report to `target/criterion/`. Open
`target/criterion/report/index.html` to browse flame charts, mean times,
and run-over-run regression deltas.

## What's measured

| File         | Covers                                                  |
|--------------|---------------------------------------------------------|
| `core.rs`    | Filesystem walk, tokenizer cost on a representative file |
| `locate.rs`  | Locator path parsing and strategy dispatch               |

System-level numbers (cold-index time on a 1M-LOC repo, query latency
under load) are not run from criterion — they live in the integration
harness described in `docs/benchmarks.md` § 3.

## Adding a benchmark

1. Add a new function to one of the bench files, or a new `*.rs` file
   and a `[[bench]]` entry in `benches/Cargo.toml`.
2. Use `criterion::black_box` on every input to keep the optimizer
   honest.
3. Pin any required fixtures inside `benches/src/lib.rs`, not in the
   bench function itself, so warm-up time is excluded.
4. Run once, commit the bench source. Run again on the reference
   hardware and update `docs/benchmarks.md` if (and only if) the change
   is intentional.

Regressions of more than 15% across two consecutive runs on the
reference hardware are treated as performance bugs — open an issue
before merging.
