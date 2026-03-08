//! Benchmarks for terminal operations.
//!
//! These benchmarks help ensure terminal performance meets targets:
//! - Character output should be fast
//! - Escape sequence parsing should handle high throughput
//! - Resize operations should be quick

use clux::Terminal;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

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

fn bench_parse_plain_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_parse");

    // Plain ASCII text (no escape sequences)
    let plain_text: Vec<u8> = (0..1000).map(|i| b'A' + (i % 26) as u8).collect();

    group.throughput(Throughput::Bytes(plain_text.len() as u64));
    group.bench_function("plain_text_1kb", |b| {
        let mut term = Terminal::new(24, 80);
        let mut parser = vte::Parser::new();

        b.iter(|| {
            parser.advance(&mut term, black_box(&plain_text));
            term.cursor.row = 0;
            term.cursor.col = 0;
        });
    });

    // Larger plain text
    let large_text: Vec<u8> = (0..100_000).map(|i| b'A' + (i % 26) as u8).collect();
    group.throughput(Throughput::Bytes(large_text.len() as u64));
    group.bench_function("plain_text_100kb", |b| {
        let mut term = Terminal::new(24, 80);
        let mut parser = vte::Parser::new();

        b.iter(|| {
            parser.advance(&mut term, black_box(&large_text));
            term.cursor.row = 0;
            term.cursor.col = 0;
        });
    });

    group.finish();
}

fn bench_parse_with_escapes(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_parse_escapes");

    // Text with color changes
    let mut colored_text = Vec::new();
    for i in 0..100 {
        // Set foreground color
        colored_text.extend_from_slice(format!("\x1b[3{}m", i % 8).as_bytes());
        colored_text.extend_from_slice(b"Hello World ");
    }

    group.throughput(Throughput::Bytes(colored_text.len() as u64));
    group.bench_function("colored_text", |b| {
        let mut term = Terminal::new(24, 80);
        let mut parser = vte::Parser::new();

        b.iter(|| {
            parser.advance(&mut term, black_box(&colored_text));
            term.cursor.row = 0;
            term.cursor.col = 0;
        });
    });

    // Text with cursor movement
    let mut cursor_text = Vec::new();
    for _ in 0..100 {
        cursor_text.extend_from_slice(b"\x1b[H"); // Home
        cursor_text.extend_from_slice(b"Line of text");
        cursor_text.extend_from_slice(b"\x1b[B"); // Down
    }

    group.throughput(Throughput::Bytes(cursor_text.len() as u64));
    group.bench_function("cursor_movement", |b| {
        let mut term = Terminal::new(24, 80);
        let mut parser = vte::Parser::new();

        b.iter(|| {
            parser.advance(&mut term, black_box(&cursor_text));
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
            term.scroll_to_bottom();
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
