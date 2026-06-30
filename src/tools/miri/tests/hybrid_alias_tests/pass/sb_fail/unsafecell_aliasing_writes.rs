#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Interior mutability — two aliasing `&UnsafeCell<i32>` references, both writing.
///
/// ## What this tests
///
/// This is the canonical interior-mutability aliasing pattern: two live shared references
/// to the same `UnsafeCell` location, both performing writes through `.get()`. Under
/// Rust's aliasing model this is valid — `UnsafeCell` is the designated escape hatch
/// that makes shared mutation sound.
///
/// Under Phase 1 tag inheritance, both `&c` reborrows return `parent_tag`
/// (`current_borrower`) unchanged instead of minting a new `shared_borrower` tag.
/// `r1` and `r2` therefore carry the same tag — the allocation's `current_borrower` —
/// and remain in `Write` state. Successive writes through either pass `check_borrower_tag`
/// because the tag matches `current_borrower`.
///
/// ## Current status
///
/// **FAILS** until Phase 1. The first write via `r1.get()` is rejected because the
/// allocation is in `BorrowerPermission::Read` and writes are unconditionally denied there.
///
/// ## After Phase 1
///
/// Both reborrows see `!Freeze` + `NewPermission::Read` → return `parent_tag`, no
/// state transition. All four accesses (two writes, two reads) succeed in `Write` state.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = UnsafeCell::new(0i32);

    // Two shared references to the same UnsafeCell — classic aliasing scenario.
    // Under Phase 1 both carry current_borrower (parent_tag); no new tag is minted.
    let r1: &UnsafeCell<i32> = &c;
    let r2: &UnsafeCell<i32> = &c;

    unsafe {
        *r1.get() = 10; // write via r1 (= current_borrower)
        *r2.get() = 20; // write via r2 (= same current_borrower) — no conflict
        let _v1 = *r1.get(); // read — still current_borrower, still Write state
        let _v2 = *r2.get();
    }

    0
}
