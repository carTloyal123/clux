//! Storage benchmarks: writing, scrolling, eviction and reflow.
//!
//! `scroll_full_screen` is the one that matters most - it is what every line of
//! shell output costs. The old grid had to shift every row and copy one into a
//! separate history buffer; the paged buffer appends a row and leaves the rest
//! where it is.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use clux::buffer::Buffer;
use clux::Cell;

fn write_screen(buffer: &mut Buffer, text: &str) {
    for row in 0..buffer.screen_rows() {
        for (col, c) in text.chars().enumerate() {
            buffer.set_cell(row, col, Cell::new(c));
        }
    }
}

fn bench_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_write");

    group.bench_function("set_cell", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        let mut col = 0;
        b.iter(|| {
            buffer.set_cell(0, col % 80, Cell::new(black_box('x')));
            col += 1;
        });
    });

    group.finish();
}

fn bench_scroll(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_scroll");

    group.bench_function("scroll_full_screen", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        write_screen(&mut buffer, "shell output line");
        b.iter(|| buffer.scroll_up());
    });

    group.bench_function("scroll_region", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        write_screen(&mut buffer, "shell output line");
        b.iter(|| buffer.scroll_region_up(2, 20));
    });

    group.bench_function("scroll_view", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        for _ in 0..1_000 {
            buffer.scroll_up();
        }
        b.iter(|| {
            buffer.scroll_view(black_box(10));
            buffer.reset_scroll();
        });
    });

    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_read");

    group.bench_function("viewport_rows_live", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        write_screen(&mut buffer, "shell output line");
        b.iter(|| {
            for row in 0..24 {
                black_box(buffer.row_cells(row));
            }
        });
    });

    group.bench_function("viewport_rows_scrolled", |b| {
        let mut buffer = Buffer::new(24, 80, 10_000);
        for _ in 0..1_000 {
            buffer.scroll_up();
        }
        buffer.scroll_view(500);
        b.iter(|| {
            for row in 0..24 {
                black_box(buffer.row_cells(row));
            }
        });
    });

    group.finish();
}

fn bench_reflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_reflow");

    group.bench_function("rewrap_1000_rows", |b| {
        b.iter_batched(
            || {
                let mut buffer = Buffer::new(24, 80, 10_000);
                for _ in 0..1_000 {
                    write_screen(&mut buffer, "a line of output to be re-wrapped");
                    buffer.scroll_up();
                }
                buffer
            },
            |mut buffer| {
                buffer.resize(24, 100, (23, 0));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_write, bench_scroll, bench_read, bench_reflow);
criterion_main!(benches);
