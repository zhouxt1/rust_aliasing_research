#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_child_unique_writer.rs
// TB: fail — ref1(Res) -> ref2(Res) -> ref3(Frz, shared). Since ref3 is frozen (read-only),
// the wildcard write can only be through ref1, disabling ref2/ref3.
// HB: fail — confirmed, but one statement earlier than TB: fails at `wild.write(42)` itself
// (ref1's tag is displaced into `reborrow_chain` by ref2's creation, and chain-only tags don't
// grant writes — same mechanism as hb_multi_exposed_child.rs), not at the later `*ref2` TB
// flags. Both fail overall.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ref1 = &mut x;
    let addr1 = (ref1 as *mut u32).expose_provenance();

    let ref2 = &mut *ref1;

    let ref3 = &*ref2;
    let _addr3 = (ref3 as *const u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr1);

    unsafe { wild.write(42) }; //~ ERROR: neither current nor prev borrower matches

    let _fail = *ref2;
    0
}
