#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/multi_exposed_child.rs
// TB: checks that when a wildcard write could be through an ancestor (ref1) or a descendant
// (ref3) of an exposed middle node (ref2), no transition is applied to ref2 since we can't
// tell which happened; ref2 stays valid. No error expected upstream.
// HB: fail — confirmed, at the very first `wild.write(42)`. ref1/ref2/ref3 form a Ref-source
// reborrow chain, so after ref3 is created, `current_borrower` = ref3's tag and `reborrow_chain`
// = [ref1_tag, ref2_tag]. `resolve_wildcard_tag` correctly finds ref1's tag (exposed) in
// `reborrow_chain` — but `check_unique_borrower_tag` only grants READS via chain-only tags, not
// writes (an existing, unrelated rule). So the wildcard write is denied even though resolution
// "succeeded". A genuine, clean HB/TB divergence: HB is stricter here, not because of anything
// wildcard-specific, but because the pre-existing reborrow_chain write-restriction applies to
// whatever tag the wildcard search happens to resolve to.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ref1 = &mut x;
    let addr1 = (ref1 as *mut u32).expose_provenance();

    let ref2 = &mut *ref1;

    let ref3 = &mut *ref2;
    let _addr3 = (ref3 as *mut u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr1);

    unsafe { wild.write(42) }; //~ ERROR: neither current nor prev borrower matches
    let _x = *ref2;
    unsafe { wild.write(43) };
    0
}
