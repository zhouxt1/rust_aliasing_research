#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::Cell;

// Port of pass/tree_borrows/cell-inside-struct.rs (utils diagnostics stripped).
// Note: HB has no "precise vs non-precise interior mutability" mode toggle the way TB does
// (-Zmiri-tree-borrows-no-precise-interior-mut) — HB always runs in one mode. So this file and
// fail/tree_borrows/cell-inside-struct.rs (already `no_per_byte`, since it's specifically about
// precise per-byte interior-mut tracking) share identical Rust code; HB's single answer is
// recorded here.
// TB (non-precise): pass — writing to field1 (frozen) is permitted when precise interior-mut
// tracking is disabled, because the whole struct gets the coarser `Cell` permission.
// TB (precise): fail — writing to field1 is forbidden (see fail/tree_borrows/cell-inside-struct).
// HB: TBD — confirmed after running; see COVERAGE.md.

struct Foo {
    field1: u32,
    field2: Cell<u32>,
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let root = Foo { field1: 42, field2: Cell::new(88) };
    unsafe {
        let a = &root;
        let a: *const Foo = a as *const Foo;
        let a: *mut Foo = a as *mut Foo;

        (*a).field2.set(10);
        (*a).field1 = 88;
    }
    0
}
