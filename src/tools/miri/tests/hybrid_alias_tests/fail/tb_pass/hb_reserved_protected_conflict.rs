#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::UnsafeCell;

// Port of the 2 "protected read" sub-functions from pass/tree_borrows/reserved.rs (utils
// diagnostic macros stripped). The other 4 sub-functions pass in HB — see
// hb_reserved_unprotected.rs.
// TB: pass — a foreign Read on a Protected Reserved reference just turns it Frozen; the
// protected reference (`x`) and the foreign pointer (`y`) are sibling Reserved nodes in the
// tree and coexist without conflict until one of them writes.
// HB: fail — confirmed on both `cell_protected_read` and `int_protected_read`, at the
// `read_second(x, y)` call site itself ("invalid parent tag ... no raw pointer stack or current
// borrower match"). `x` and `y` are both `&mut *base` reborrows of the same raw pointer `base`,
// created sequentially; per `apply_reborrow_to_stack`'s "second reborrow from the same base"
// rule (added to fix `hb_wildcard_sibling_disable.rs`), `y`'s creation overwrites the
// `exposed_stack` entry in place, displacing `x`'s tag entirely — before `x` is ever passed to
// `read_second`, `x`'s tag is no longer live anywhere in `BorrowerState`. This is a genuine
// HB/TB divergence surfaced by that earlier fix: HB has no tree, so it cannot let two sibling
// reborrows of the same raw pointer coexist the way TB's Reserved nodes do — the second one
// unconditionally displaces the first, regardless of whether either side has written yet.

unsafe fn read_second<T>(x: &mut T, y: *mut u8) {
    let _keep_alive = x as *mut T;
    let _val = *y;
}

// Foreign Read on a interior mutable Protected Reserved turns it Frozen (TB); must not be UB.
unsafe fn cell_protected_read() {
    let base = &mut UnsafeCell::new(0u8);
    let x = &mut *(base as *mut UnsafeCell<u8>);
    let y = &mut *base as *mut UnsafeCell<u8> as *mut u8;
    read_second(x, y); //~ ERROR: invalid parent tag
}

// Foreign Read on a Protected Reserved turns it Frozen (TB); must not be UB.
unsafe fn int_protected_read() {
    let base = &mut 0u8;
    let x = &mut *(base as *mut u8);
    let y = (&mut *base) as *mut u8;
    read_second(x, y); //~ ERROR: invalid parent tag
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        cell_protected_read();
        int_protected_read();
    }
    0
}
