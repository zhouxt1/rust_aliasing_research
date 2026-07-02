#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_siblings_local.rs
// TB: fail — wildcard write activates ptr_base; a parent read of `x` freezes ptr_base (since it
// was Active); a second wildcard write then fails because ptr_base is Frozen.
// HB: pass — confirmed exactly as predicted. ref1/ref2 are sequential RawPtr-source siblings, so
// only ref2 is live after creation (exposed_stack top), but ref2 was also exposed, so the
// wildcard resolves to ref2 both times. The parent read `let _y = x;` has NO effect on children
// in HB (an already-documented property: parent reads don't freeze/kill), so the second wildcard
// write succeeds where TB's freezes it — a clean, expected divergence, not a new mechanism.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &mut *ptr_base };

    let addr1 = (ref1 as *mut u32).expose_provenance();
    let _addr2 = (ref2 as *mut u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr1);

    unsafe { wild.write(41) };

    let _y = x;

    unsafe { wild.write(0) };
    0
}
