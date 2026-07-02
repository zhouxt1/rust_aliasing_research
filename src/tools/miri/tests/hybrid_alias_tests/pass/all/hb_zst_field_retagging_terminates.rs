#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of pass/stacked_borrows/zst-field-retagging-terminates.rs
// SB & TB: pass — retagging `[(); usize::MAX]` must terminate quickly via a ZST fast path; no
// aliasing logic is actually exercised (a ZST array occupies zero bytes).
// HB: this is a pure termination/performance check, not an aliasing-model divergence — if HB's
// retag logic ever iterates per-element for arrays without a ZST short-circuit, this would hang
// effectively forever (usize::MAX iterations). Confirmed via run_tests.sh with a bounded timeout.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let array = [(); usize::MAX];
    drop(array);
    0
}
