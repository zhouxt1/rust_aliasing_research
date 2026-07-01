#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/unescaped_local.rs
// SB: fail — the wildcard pointer's original tag is popped from the borrow stack once `_ptr`
//     reborrows `x`; write through the wildcard is denied.
// HB: fail — the exposed tag (from the first `&mut x as *mut i32` retag) is displaced by the
//     `_ptr = &mut x` reborrow and is not retained anywhere live in `BorrowerState` (Ref-source
//     reborrows only push the old tag into `reborrow_chain` when no `exposed_stack` exists;
//     here it does not survive at all in this MIR shape). `resolve_wildcard_tag` finds no live
//     tag that is also in `exposed_tags`, so the access is denied outright — converging with SB.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 42i32;
    unsafe {
        // Cast to integer (exposes provenance) and back — creates a wildcard pointer.
        let addr = (&mut x as *mut i32).expose_provenance();
        let raw = core::ptr::with_exposed_provenance_mut::<i32>(addr);
        // Create a new reborrow — displaces raw's original tag.
        let _ptr = &mut x;
        // SB: denied (raw's tag popped from stack).
        // HB: denied (raw's exposed tag is no longer live anywhere in BorrowerState).
        *raw = 13; //~ ERROR: no exposed tags are currently live
    }
    0
}
