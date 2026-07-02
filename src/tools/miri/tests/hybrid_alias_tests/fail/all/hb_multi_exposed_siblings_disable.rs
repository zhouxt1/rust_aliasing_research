#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_siblings_disable.rs
// TB: fail — ref1, ref2, ref3 are siblings from ptr_base; ref1 and ref2 are exposed. Writing
// through ref3 disables ref1/ref2 (siblings). Both exposed refs now disabled, so the wildcard
// access finds no valid candidate.
// HB: fail — confirmed exactly as predicted. ref1/ref2/ref3 are sequential RawPtr-source
// reborrows of the same ptr_base; per the sibling-reborrow-displacement rule, each reborrow
// displaces the prior one from `exposed_stack`. After ref3 is created, only ref3's tag is live —
// but ref3 was never exposed. `resolve_wildcard_tag` finds no live-and-exposed tag, so the
// wildcard read is denied ("no exposed tags are currently live").

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &mut *ptr_base };
    let ref3 = unsafe { &mut *ptr_base };

    let addr1 = (ref1 as *mut u32).expose_provenance();
    let _addr2 = (ref2 as *mut u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr1);

    *ref3 = 13;

    let _fail = unsafe { *wild }; //~ ERROR: no exposed tags are currently live
    0
}
