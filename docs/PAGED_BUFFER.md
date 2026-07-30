# One paged buffer, screen as a window

`Grid` + `Scrollback` are gone: terminal content lives in a single paged buffer
(`src/buffer/`), following the model Ghostty uses, with the screen as a window into
it. This records the design, what was deliberately not copied, and what it measured.

## Why

Clux used to store terminal content in two unrelated containers: `Grid` (fixed
`rows × cols`, mutable, indexed by screen position) and `Scrollback` (ring of
lines, immutable, indexed by age). Every feature that spanned them paid for it:

- **Resize did not re-wrap history.** Scrollback lines kept the width they were
  recorded at, so widening the window left old output wrapped at the old column.
  It re-wraps now.
- **Selection could not cross the scroll boundary**, because the two halves were
  different types with different coordinate systems. They are one type now, though
  selection still stops at the viewport edge (see SELECTION.md).
- **Link resolution needed a whole abstraction** (`RowSource`) to see both halves.
  With one storage model it takes the buffer directly, and the trait is deleted.
- Two containers means two implementations of anything row-shaped, which is how
  this repo previously ended up with three ANSI emitters.

## What Ghostty does

Ghostty stores everything in a `PageList`: a doubly-linked list of page-aligned
memory blocks, "the first page is the topmost page (scrollback) and the last is
the bottommost page (the current active page)". The *active* area (the screen) is
simply the last rows of that list; *history* is everything before it; the
*viewport* is what is currently displayed and can be pinned to `active`, `top`, or
an arbitrary tracked position.

Key mechanisms:

- **Page** — one contiguous allocation holding a rows array and a cells array
  (`Row` and `Cell` are each 64 bits, packed). Rows reference their cells by
  *offset*, not pointer, "so we can do a simple linear copy of the backing memory
  and copy all the offsets and everything will work".
- **Interning** — cells carry a style id and hyperlink id, not values. Styles live
  in a per-page refcounted set; hyperlink URIs are interned strings. This is why
  their cell is 8 bytes.
- **Pins** — a `Pin` is `(node, x, y)`, registered in a `tracked_pins` set that is
  updated automatically as pages change. The cursor is a pin; selection endpoints
  are pins. That is how both survive scrolling, eviction and reflow.
- **Reflow** — `resizeCols` walks content through a `ReflowCursor` into fresh
  pages, remapping tracked pins as it goes; column reflow can change the physical
  row count of history.
- **Budget in bytes** — `max_scrollback_bytes` (default 10 MB), with only whole
  historical pages pruned. Line count is a secondary, optional limit.
- **Idle compression** — history pages are LZ4-compressed on a 250 ms idle
  debounce and their physical pages dropped with `madvise`, measured at ~6% of
  original size, ~101 µs per page.

## What clux took, and what it did not

Taken:

- One buffer; the screen is a window into it. This is the whole point.
- Pages as the allocation unit: O(1) eviction of history, ~80 allocations for a
  10k-row history instead of 10,000.
- Pins, so the viewport (and later selection anchors) survive scrolling, eviction
  and reflow.
- Fixed-width rows *within* a page. Uniform addressing keeps reflow and iteration
  simple; variable-length trimmed rows would make in-place writes impossible.
- A byte budget rather than `max_lines = 10_000`, which is an arbitrary number
  that says nothing about memory.

Skipped, deliberately:

- **Offsets-instead-of-pointers.** That exists so pages can be `memcpy`'d and
  mmap'd. In Rust, indices into a page's arrays are already relocatable.
- **Grapheme and wide-character storage.** Clux is one `char` per cell today. That
  is a real limitation and a separate project; the design must not preclude it.
- **Compression / `madvise`.** Only worth it after cells shrink, needs `unsafe` and
  per-OS code. Revisit with a measurement.
- **Style interning.** The path from a 24-byte cell to ~8, but a memory change
  rather than a correctness one - see "Cell size" below.
- **A pinned cursor.** Ghostty pins the cursor because its pages rotate. Escape
  sequences address the cursor in *screen* coordinates, and the active area is
  always the last rows, so screen-relative addressing is already stable under
  scrolling. Pins are for the viewport, which genuinely has to hold a position in
  history.

## Design as built

```rust
// src/buffer/
pub struct Buffer {
    pages: VecDeque<Page>,   // front = oldest history, back = active area
    cols: usize,
    screen_rows: usize,      // the active window height
    first_abs: u64,          // absolute row number of the oldest live row
    front_skip: usize,       // evicted rows still held by the first page
    stored_rows: usize,
    history_bytes: usize,    // memory budget; 0 for the alt screen
    viewport: Viewport,
}

struct Page {                // one allocation per ROWS_PER_PAGE (128) rows
    cols: usize,
    meta: Vec<RowMeta>,      // wrapped + dirty, per row
    cells: Vec<Cell>,        // meta.len() * cols, fixed width
}

pub struct Pin(u64);                       // absolute row number
enum Viewport { Active, Pinned(Pin) }
```

Three decisions worth stating, all departures from Ghostty:

1. **A pin is an absolute row number, not `(page, row)`.** Rows are numbered
   monotonically for the life of the pane. Eviction is then one integer update with
   no pin fixups, where Ghostty walks a tracked-pin set. Reflow still remaps pins
   explicitly - unavoidable either way - and renumbers past every old row, so a pin
   taken before a reflow resolves to nothing rather than to the wrong row.
2. **No linked list.** `VecDeque<Page>` gives O(1) push/pop at both ends, which is
   all the list was for.
3. **Evicted rows are skipped, not removed.** `front_skip` counts dead rows at the
   front of the first page, so every page except the last holds exactly
   `ROWS_PER_PAGE` rows - which is what lets a row be located by division instead of
   walking the pages. At most one page's worth of rows is held this way. This was not
   a design choice up front; the bench found the walk.

Consequences elsewhere:

- `Terminal` holds `buffer` plus `alt_primary: Option<Buffer>`; entering the alt
  screen swaps in a buffer with a zero history budget, replacing the old
  `alt_grid: Option<Grid>`.
- `RowSource` is **deleted**. It existed only to let link resolution read two
  different containers; with one, `resolve_links` takes `&Buffer`.
- Height resize moves the active/history boundary: growing pulls history back into
  view, shrinking pushes the top of the screen into history, and blank rows below the
  cursor are reclaimed first so shrinking a mostly-empty screen does not shove live
  content into history. Width resize re-wraps and keeps the cursor's screen row.
- The wire protocol did not change: updates stay viewport-row indexed.

## Staging

Each stage ends with the whole suite green and nothing half-migrated.

| Stage | Work | Outcome |
| --- | --- | --- |
| 0 | Invariant tests for wrap/scroll/resize against the public API, plus a bench baseline | `tests/scroll_invariants.rs`, `tests/content_invariants.rs` - 10 tests that pinned behaviour through the swap |
| 1 | `src/buffer/`: `Page`, `Buffer`, `Pin`, viewport | Done; 39 unit tests |
| 2 | Port `Terminal`; delete `grid.rs`, `scrollback.rs`, `scrollview/`, `rowsource.rs` | Done; whole suite passed unchanged at the checkpoint |
| 3 | Reflow across the whole buffer, history included | Done; folded into stage 2 so resize never regressed in between |
| 4 | Byte budget, whole-page eviction | Done; `DEFAULT_SCROLLBACK_BYTES` = 16 MB, derived row limit |
| 5 | Shrink `Cell` | Partly: 24 → 20 bytes via a `NonZeroU32` niche. Interning deferred - see below |

## Measured

Same benches, old implementation vs new (`cargo bench --bench terminal`):

| Bench | Before | After | |
| --- | --- | --- | --- |
| `linefeed_at_bottom` | 508 ns | **76 ns** | 6.7× faster |
| `parse/plain_text_100kb` | 875 µs | **463 µs** | 1.9× faster |
| `scroll_view/scroll_up` | 17.1 ns | **1.3 ns** | 13× faster |
| `put_char/single_char` | 3.57 ns | 4.78 ns | 34% slower |
| `parse/plain_text_1kb` | 3.86 µs | 5.01 µs | 30% slower |
| `scroll_view/scroll_down` | 17.1 ns | 75.5 ns | 4.4× slower |

The trade is structural: a full-screen scroll is now "append one row" instead of
"shift every row and copy one into a second container", so anything that scrolls -
which is any output taller than the screen - gets much cheaper. A single cell write
costs ~1.2 ns more (a division and a deque index instead of a `Vec` index), so short
bursts that fit on screen are slower. Two attempted fixes made it worse and were
reverted; the remaining gap is not worth more machinery.

Two regressions found *by* these benches, both fixed: row addressing walked the page
list (O(pages) per cell write), and `mark_all_dirty` marked every row twice.

## Cell size

`Cell` is 20 bytes: `char` 4 + two `Color` 4 + flags 1 + `Option<HyperlinkId>` 4,
padded. A `NonZeroU32` id took it from 24, which is 17% off every cell everywhere.

Going further means one of:

- **Pack the two `ColorKind` tags into the flags byte** → 16 bytes (another 20%). No
  new machinery, but it touches every colour construction and read.
- **Per-page style interning**, as Ghostty does → ~8 bytes. This is the big one, and
  the reason it is deferred rather than done: protocol cells carry colour *values*,
  so every serialization boundary would have to resolve style ids back to values.
  That is a protocol and client change, not a storage change, and it should be
  justified by a real memory problem rather than by symmetry with Ghostty.

## Risks

- **Reflow correctness.** Mitigated by property tests: reflow to width W and back
  preserves logical content; a tracked cursor stays on the same character; wrap
  flags round-trip.
- **Write-path throughput.** The VTE path is the hot loop. Pin→cell must stay
  arithmetic, and the stage 0 bench is the guard.
- **Dirty tracking.** Per-row flags move into `RowMeta`; the risk is missed
  repaints, which the integration tests exercise (scroll, type, split, resize).
- **Alt screen transitions.** Entering and leaving the alt screen must not touch
  primary history; needs explicit tests.

## Payoff

Resize re-wraps history. Selection spans the scroll boundary. Links in history are
structural rather than a special case. Eviction is O(1). One storage model instead
of two, and `grid.rs` + `scrollback.rs` + `scrollview/` (~1,170 lines) collapse
into one module. Memory work becomes possible afterwards.
