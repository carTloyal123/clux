//! Benchmarks for terminal operations.
//!
//! These benchmarks help ensure terminal performance meets targets:
//! - Character output should be fast
//! - Escape sequence parsing should handle high throughput
//! - Resize operations should be quick

use clux::Terminal;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

mod parse;
use parse::*;

fn bench_put_char(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_put_char");
    group.throughput(Throughput::Elements(1));

    let mut term = Terminal::new(24, 80);

    group.bench_function("single_char", |b| {
        b.iter(|| {
            term.put_char(black_box('A'));
            // Reset cursor periodically to avoid scrolling
            if term.cursor.col >= 79 {
                term.cursor.col = 0;
                if term.cursor.row >= 23 {
                    term.cursor.row = 0;
                }
            }
        });
    });

    group.finish();
}

fn bench_resize(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_resize");

    for (rows, cols) in [(24, 80), (48, 120), (100, 200)] {
        let size_str = format!("{}x{}", cols, rows);
        group.bench_with_input(
            BenchmarkId::from_parameter(&size_str),
            &(rows, cols),
            |b, &(rows, cols)| {
                let mut term = Terminal::new(24, 80);

                // Fill with some content
                for _ in 0..24 {
                    for c in "Hello, World! ".chars() {
                        term.put_char(c);
                    }
                }

                b.iter(|| {
                    term.resize(black_box(rows), black_box(cols));
                    term.resize(24, 80); // Reset for next iteration
                });
            },
        );
    }

    group.finish();
}

fn bench_scroll(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_scroll");

    group.bench_function("linefeed_at_bottom", |b| {
        let mut term = Terminal::new(24, 80);

        // Fill terminal
        for row in 0..24 {
            term.cursor.row = row;
            term.cursor.col = 0;
            for c in "Line of text content here".chars() {
                term.put_char(c);
            }
        }

        b.iter(|| {
            term.cursor.row = 23;
            term.linefeed();
        });
    });

    group.finish();
}

fn bench_scroll_view(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_scroll_view");

    // Create terminal with scrollback
    let mut term = Terminal::new(24, 80);

    // Generate scrollback content
    for _ in 0..1000 {
        term.cursor.row = 23;
        term.linefeed();
        for c in "Scrollback line content".chars() {
            term.put_char(c);
        }
    }

    group.bench_function("scroll_up", |b| {
        b.iter(|| {
            term.scroll_view(black_box(-10));
            term.reset_scroll();
        });
    });

    group.bench_function("scroll_down", |b| {
        term.scroll_view(-500); // Scroll up first
        b.iter(|| {
            term.scroll_view(black_box(10));
            term.scroll_view(-10); // Reset
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_put_char,
    bench_parse_plain_text,
    bench_parse_with_escapes,
    bench_resize,
    bench_scroll,
    bench_scroll_view
);
criterion_main!(benches);
