#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::ptr::addr_of_mut;

// MOVED (2026-07-02), status UNCERTAIN — see COVERAGE.md's "pass/sb_tb_fail case study". This
// test used to be a *documented, deliberate* divergence: HB passed because it has no
// TB-specific "conflicted" flag concept. After the `RawPointerStack.dead` fix, HB now fails
// too, converging with TB on the verdict — but this is very likely ACCIDENTAL, a side effect of
// the same blunt "any base-pointer read kills the child entry" rule, not a genuine
// implementation of TB's conflicted-flag semantics. Do not treat this as "fixed" the way the
// other 5 pass/sb_tb_fail tests are; the underlying mechanism HB now uses to reject this program
// is unrelated to *why* TB rejects it. Revisit once/if the `activated` field is implemented —
// this test's outcome may flip back to pass, since `foo`/`x` here are never activated either.
//
// Port of fail/tree_borrows/repeated_foreign_read_lazy_conflicted.rs
// TB: fail — a foreign read sets the "conflicted" flag on the lazy part of a Reserved
// reference; a subsequent write through the conflicted pointer is forbidden.
// HB: **fail** (post-fix) — but see the UNCERTAIN note above; likely accidental convergence.

fn do_something(_: u8) {}

unsafe fn access_after_sub_1(x: &mut u8, orig_ptr: *mut u8) {
    do_something(*orig_ptr);
    *(x as *mut u8).byte_sub(1) = 42;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let mut alloc = [0u8, 0u8];
        let orig_ptr = addr_of_mut!(alloc) as *mut u8;
        let foo = &mut *orig_ptr;
        do_something(alloc[0]);
        access_after_sub_1(&mut *(foo as *mut u8).byte_add(1), orig_ptr);
    }
    0
}
