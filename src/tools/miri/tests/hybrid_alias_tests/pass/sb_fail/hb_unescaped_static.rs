#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/unescaped_static.rs
// SB: fail — a tag derived from `&ARRAY[0]` is only ever pushed onto byte 0's own borrow stack;
// SB tracks one stack PER BYTE, so accessing byte 1 with that tag is "tag does not exist in the
// borrow stack".
// HB: pass — confirmed. HB's BorrowerState is per-ALLOCATION, not per-byte (the existing
// `no_per_byte` limitation), so the tag from `&ARRAY[0]` remains valid for accessing byte 1
// of the same allocation. This is the static-array instance of a general, already-documented
// HB/SB divergence — not a static-specific feature gap.

static ARRAY: [u8; 2] = [0, 1];

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let ptr_to_first = &ARRAY[0] as *const u8;
    // Illegally (per SB) use this to access the 2nd element.
    let _val = unsafe { *ptr_to_first.add(1) };
    0
}
