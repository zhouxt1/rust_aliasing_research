#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::mem;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Transmuting between `&i32` and `&UnsafeCell<i32>` for read-only accesses.
///
/// ## Source
/// Ported from `tree_borrows/pass/transmute-unsafecell.rs`.
/// That test passes SB and TB. HB **fails** this due to a PoloniusAnchor implementation bug.
///
/// ## What this tests
///
/// `ref_to_cell`: `let x: &i32 = &val; let cell_x: &UnsafeCell<i32> = transmute(x);`
/// then reads via `*cell_x.get()`. The behavior is safe — read-only access through
/// a transmuted shared reference should be permitted.
///
/// ## HB Limitation (why this is in `pass/broken/`)
///
/// `mem::transmute(x)` CONSUMES `x: &i32`. When `x` is moved into transmute, the NLL
/// scheduler ends `x`'s borrow of `val`. The PoloniusPass inserts an anchor for `x`
/// that fires here; afterward, Miri's `StorageDead` for `val` may execute before
/// `*cell_x.get()` runs — Miri then flags this as "accessing a dead local variable."
///
/// Root cause: HB's PoloniusAnchor fires when the ORIGINAL reference type's lifetime ends
/// (at the transmute site), not when the TRANSMUTED reference actually goes out of scope.
/// The anchor mechanism does not account for lifetime extension through `mem::transmute`.
///
/// Workaround: use a raw pointer cast (`&*(x as *const i32 as *const UnsafeCell<i32>)`)
/// instead of `mem::transmute`. The raw cast does not consume `x`, keeping the anchor
/// tied to `x`'s actual scope end.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **pass**| SharedReadOnly tag allows reads; transmute preserves provenance |
/// | TB    | **pass**| Read-only accesses; transmuted ref is still in valid tree state |
/// | HB    | **fail (bug)**| PoloniusAnchor fires at transmute site; Miri flags val as dead |
unsafe fn ref_to_cell() -> i32 {
    let val: i32 = 42;
    let x: &i32 = &val;
    let cell_x: &UnsafeCell<i32> = mem::transmute(x);
    *cell_x.get()
}

unsafe fn cell_to_ref() -> i32 {
    let x = UnsafeCell::new(42i32);
    let ref_x: &i32 = mem::transmute(&x);
    *ref_x
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let _a = ref_to_cell();
        let _b = cell_to_ref();
    }
    0
}
