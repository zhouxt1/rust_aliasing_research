#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_siblings_foreign.rs
// TB: fail — ref1, ref2, ref3 are siblings; ref1/ref2 exposed. Writing through the wildcard
// (resolves to ref1 or ref2) disables ref3, a sibling foreign to both. `*ref3` then fails.
// HB: fail — confirmed, one statement earlier than TB: fails at `wild.write(13)` itself, not
// at the later `*ref3`. Sequential RawPtr-source siblings mean only ref3 is live after creation
// (ref1/ref2 already displaced by the sibling-reborrow rule and never exposed), so the wildcard
// write finds no exposed-and-live tag. Same overall verdict (fail) as TB, different statement.

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

    unsafe { wild.write(13) }; //~ ERROR: no exposed tags are currently live

    let _fail = *ref3;
    0
}
