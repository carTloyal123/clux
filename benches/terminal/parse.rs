//! Parser throughput benchmarks.

//! Benchmarks for terminal operations.
//!
//! These benchmarks help ensure terminal performance meets targets:
//! - Character output should be fast
//! - Escape sequence parsing should handle high throughput
//! - Resize operations should be quick

use clux::Terminal;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

pub fn bench_parse_plain_text(c: &mut Criterion) {
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

pub fn bench_parse_with_escapes(c: &mut Criterion) {
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
