#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of `aliasing_read_only_mutable_refs` from pass/tree_borrows/tree-borrows.rs.
// TB: pass — TB has no issue with several mutable references existing at the same time, as
// long as they are used only immutably. Multiple Reserved nodes can coexist as siblings.
// HB: fail — confirmed. `r1` and `r2` are both `&mut *(base as *mut u64)` reborrows of the same
// raw pointer, created sequentially. Per `apply_reborrow_to_stack`'s "second reborrow from the
// same base" rule (added to fix hb_wildcard_sibling_disable.rs), `r2`'s creation overwrites the
// `exposed_stack` entry in place, displacing `r1`'s tag before `*r1` is ever read. Same root
// cause as hb_reserved_protected_conflict.rs — HB has no tree, so it cannot let two sibling
// reborrows of the same raw pointer coexist read-only the way TB's Reserved nodes do.

fn aliasing_read_only_mutable_refs() {
    unsafe {
        let base = &mut 42u64;
        let r1 = &mut *(base as *mut u64);
        let r2 = &mut *(base as *mut u64);
        let _l = *r1; //~ ERROR: no raw pointer stack or current borrower match
        let _l = *r2;
    }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    aliasing_read_only_mutable_refs();
    0
}
