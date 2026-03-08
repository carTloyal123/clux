//! Benchmarks for grid operations.
//!
//! These benchmarks help ensure grid performance meets targets:
//! - Cell access should be O(1)
//! - Scroll operations should be efficient
//! - Dirty tracking should have minimal overhead

use clux::{Cell, Grid};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

fn bench_get_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_get_set");

    for (rows, cols) in [(24, 80), (48, 120), (100, 200)] {
        let mut grid = Grid::new(rows, cols);
        let size_str = format!("{}x{}", cols, rows);

        group.bench_with_input(
            BenchmarkId::new("set", &size_str),
            &(rows, cols),
            |b, &(rows, cols)| {
                let cell = Cell::default();
                let mut row = 0;
                let mut col = 0;

                b.iter(|| {
                    grid.set(row, col, black_box(cell));
                    col += 1;
                    if col >= cols {
                        col = 0;
                        row = (row + 1) % rows;
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("get", &size_str),
            &(rows, cols),
            |b, &(rows, cols)| {
                let mut row = 0;
                let mut col = 0;

                b.iter(|| {
                    let _ = black_box(grid.get(row, col));
                    col += 1;
                    if col >= cols {
                        col = 0;
                        row = (row + 1) % rows;
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_scroll_up(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_scroll_up");

    for (rows, cols) in [(24, 80), (48, 120), (100, 200)] {
        let mut grid = Grid::new(rows, cols);
        let size_str = format!("{}x{}", cols, rows);

        // Fill with content
        for row in 0..rows {
            for col in 0..cols {
                let mut cell = Cell::default();
                cell.c = (b'A' + (col % 26) as u8) as char;
                grid.set(row, col, cell);
            }
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(&size_str),
            &(rows, cols),
            |b, _| {
                b.iter(|| {
                    let _ = black_box(grid.scroll_up());
                });
            },
        );
    }

    group.finish();
}

fn bench_scroll_region(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_scroll_region");

    let mut grid = Grid::new(24, 80);

    // Fill with content
    for row in 0..24 {
        for col in 0..80 {
            let mut cell = Cell::default();
            cell.c = (b'A' + (col % 26) as u8) as char;
            grid.set(row, col, cell);
        }
    }

    group.bench_function("scroll_region_up", |b| {
        b.iter(|| {
            grid.scroll_region_up(black_box(5), black_box(20));
        });
    });

    group.finish();
}

fn bench_resize(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_resize");

    for (from_rows, from_cols, to_rows, to_cols) in [
        (24, 80, 48, 120), // Grow
        (48, 120, 24, 80), // Shrink
        (24, 80, 24, 120), // Widen only
        (24, 80, 48, 80),  // Taller only
    ] {
        let size_str = format!("{}x{}_to_{}x{}", from_cols, from_rows, to_cols, to_rows);

        group.bench_with_input(
            BenchmarkId::from_parameter(&size_str),
            &(from_rows, from_cols, to_rows, to_cols),
            |b, &(from_rows, from_cols, to_rows, to_cols)| {
                b.iter(|| {
                    let mut grid = Grid::new(from_rows, from_cols);

                    // Fill with content
                    for row in 0..from_rows {
                        for col in 0..from_cols {
                            let mut cell = Cell::default();
                            cell.c = 'X';
                            grid.set(row, col, cell);
                        }
                    }

                    grid.resize(black_box(to_rows), black_box(to_cols));
                });
            },
        );
    }

    group.finish();
}

fn bench_dirty_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_dirty_tracking");

    let mut grid = Grid::new(24, 80);

    group.bench_function("mark_dirty", |b| {
        let mut row = 0;
        b.iter(|| {
            if let Some(r) = grid.row_mut(row) {
                r.mark_dirty();
            }
            row = (row + 1) % 24;
        });
    });

    group.bench_function("check_dirty", |b| {
        // Mark some rows dirty
        for i in 0..12 {
            if let Some(r) = grid.row_mut(i * 2) {
                r.mark_dirty();
            }
        }

        b.iter(|| {
            let _ = black_box(grid.has_dirty_rows());
        });
    });

    group.bench_function("dirty_indices", |b| {
        // Mark some rows dirty
        for i in 0..12 {
            if let Some(r) = grid.row_mut(i * 2) {
                r.mark_dirty();
            }
        }

        b.iter(|| {
            let indices: Vec<_> = grid.dirty_row_indices().collect();
            black_box(indices);
        });
    });

    group.finish();
}

fn bench_clear(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_clear");

    for (rows, cols) in [(24, 80), (48, 120)] {
        let mut grid = Grid::new(rows, cols);
        let size_str = format!("{}x{}", cols, rows);

        // Fill with content
        for row in 0..rows {
            for col in 0..cols {
                let mut cell = Cell::default();
                cell.c = 'X';
                grid.set(row, col, cell);
            }
        }

        group.bench_with_input(
            BenchmarkId::new("clear_row", &size_str),
            &(rows, cols),
            |b, _| {
                let mut row = 0;
                b.iter(|| {
                    if let Some(r) = grid.row_mut(row) {
                        r.clear();
                    }
                    row = (row + 1) % rows;
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_get_set,
    bench_scroll_up,
    bench_scroll_region,
    bench_resize,
    bench_dirty_tracking,
    bench_clear
);
criterion_main!(benches);
