//! Benchmarks for scrollback buffer operations.
//!
//! These benchmarks help ensure scrollback performance meets targets:
//! - Push operations should be O(1)
//! - Get operations should be O(1)
//! - Memory usage should scale linearly

use clux::{Cell, Scrollback};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// Create a vector of cells representing a line of text.
fn make_line(width: usize) -> Vec<Cell> {
    (0..width)
        .map(|i| {
            let mut cell = Cell::default();
            cell.c = (b'A' + (i % 26) as u8) as char;
            cell
        })
        .collect()
}

fn bench_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_push");

    for size in [1_000, 10_000, 50_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let mut sb = Scrollback::new(size);
            let line = make_line(80);

            b.iter(|| {
                sb.push(black_box(line.clone()), false);
            });
        });
    }

    group.finish();
}

fn bench_push_at_capacity(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_push_at_capacity");

    for size in [10_000, 50_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            // Pre-fill to capacity
            let mut sb = Scrollback::new(size);
            let line = make_line(80);
            for _ in 0..size {
                sb.push(line.clone(), false);
            }

            b.iter(|| {
                sb.push(black_box(line.clone()), false);
            });
        });
    }

    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_get");

    for size in [1_000, 10_000, 50_000, 100_000] {
        // Pre-fill scrollback
        let mut sb = Scrollback::new(size);
        let line = make_line(80);
        for _ in 0..size {
            sb.push(line.clone(), false);
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let mut offset = 0usize;
            b.iter(|| {
                let _ = black_box(sb.get(offset % size));
                offset = offset.wrapping_add(1);
            });
        });
    }

    group.finish();
}

fn bench_extract_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_extract_text");

    for size in [100, 1_000, 10_000] {
        // Pre-fill scrollback
        let mut sb = Scrollback::new(size);
        let line = make_line(80);
        for _ in 0..size {
            sb.push(line.clone(), false);
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                black_box(sb.extract_text(0, size));
            });
        });
    }

    group.finish();
}

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_search");

    for size in [1_000, 10_000, 50_000] {
        // Pre-fill scrollback with searchable content
        let mut sb = Scrollback::new(size);
        for i in 0..size {
            let text = format!("Line {} with some searchable content", i);
            let cells: Vec<Cell> = text
                .chars()
                .map(|c| {
                    let mut cell = Cell::default();
                    cell.c = c;
                    cell
                })
                .collect();
            sb.push(cells, false);
        }

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                black_box(sb.search("searchable"));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_push,
    bench_push_at_capacity,
    bench_get,
    bench_extract_text,
    bench_search
);
criterion_main!(benches);
