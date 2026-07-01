#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of the `static_memory_modification` sub-module in pass/tree_borrows/sb_fails.rs.
// SB: fail — same eager retag-time failure as the basic static_memory_modification test.
// TB: pass — TB's reborrow is lazy (Reserved state); since this code only READS through the
// transmuted reference and never writes, TB's deferred permission check never fires.
// HB: fail — confirmed. HB's retag rejects creating a non-shared (`&mut`) reference from an
// already-shared tag eagerly, at the transmute itself, regardless of whether the resulting
// reference is ever written through. So HB sides with SB's eager-failure semantics here, not
// TB's lazy/deferred one — a genuine three-way divergence point distinct from the basic
// static_memory_modification test (where SB/HB both fail, just via different mechanisms).

static X: usize = 5;

#[no_mangle]
#[allow(mutable_transmutes)]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let x = unsafe { core::mem::transmute::<&usize, &mut usize>(&X) }; //~ ERROR: non-shared reference using a shared tag
    // TB tolerates this transmute as long as no write occurs — only a read happens here.
    let v = *&*x;
    v as isize
}
