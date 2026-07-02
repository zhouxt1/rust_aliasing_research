#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of pass/tree_borrows/copy-nonoverlapping.rs
// SB: fail on test_from_to — as_mut_ptr's reborrow invalidates the earlier pointer from as_ptr.
// TB: pass — both orderings work regardless of which pointer (shared or mutable) is obtained
// first.
// HB: fail on test_from_to, confirmed (test_to_from alone passes) — `data.as_ptr()` creates a
// transient `&self` reference that HB's Polonius tracking keeps alive as a `shared_borrower`
// past the statement boundary; the later `data.as_mut_ptr()` needing a non-shared tag then
// conflicts with it ("Attempting to create a non-shared reference using a shared tag"). HB does
// NOT achieve TB's order-independence here — it converges with SB's failure on this ordering
// instead, for an unrelated (Polonius-lingering-borrow vs. raw-provenance-restriction) reason.
// Since the combined test (both orderings) does not fully pass, this is `fail/tb_pass/`, not a
// clean `covered`/`pass`.

fn test_to_from() {
    unsafe {
        let data = &mut [0u64, 1];
        let to = data.as_mut_ptr().add(1);
        let from = data.as_ptr();
        core::ptr::copy_nonoverlapping(from, to, 1);
    }
}

fn test_from_to() {
    unsafe {
        let data = &mut [0u64, 1];
        let from = data.as_ptr();
        let to = data.as_mut_ptr().add(1); //~ ERROR: non-shared reference using a shared tag
        core::ptr::copy_nonoverlapping(from, to, 1);
    }
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    test_to_from();
    test_from_to();
    0
}
