#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_siblings_unique_writer.rs
// TB: fail — ref1(Res, mutable) and ref2(Frz, shared) are both exposed; only ref1 can write,
// so the wildcard write disables ref2 (the frozen sibling). `*ref2` then fails.
// HB: fail — confirmed, but via a different mechanism than TB: the wildcard resolves to ref2's
// tag (a shared/read-only tag), and HB denies the write outright ("write access on a shared
// borrow with tag"), since Read-permission state never grants writes regardless of exposure.
// Both fail overall, but HB's reasoning is about permission class, not disabling a sibling.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &*ptr_base };

    let addr1 = (ref1 as *mut u32).expose_provenance();
    let _addr2 = (ref2 as *const u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr1);

    unsafe { wild.write(13) }; //~ ERROR: write access on a shared borrow

    let _fail = *ref2;
    0
}
