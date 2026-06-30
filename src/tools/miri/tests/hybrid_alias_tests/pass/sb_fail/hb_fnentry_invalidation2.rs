#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Calling `as_mut_ptr()` on a slice in an inner function does not invalidate an outer
/// raw pointer in HB, unlike in Stacked Borrows.
///
/// ## Source
/// Ported from `stacked_borrows/fail/fnentry_invalidation2.rs`.
/// That test fails SB. HB and TB both pass.
///
/// ## What this tests
///
/// ```rust
/// let mut arr = [0i32, 1, 2];
/// let ptr = arr.as_ptr();        // *const i32 derived from arr's allocation
/// inner(&mut arr);
/// let _oof = unsafe { *ptr };    // SB: ptr's tag was invalidated by inner's as_mut_ptr()
///
/// fn inner(sli: &mut [i32]) {
///     let _ = sli.as_mut_ptr();  // In SB: Unique reborrow pops ptr's SRW tag
/// }
/// ```
///
/// ## SB behavior
///
/// `arr.as_ptr()` pushes SRW(T_ptr) above Unique(T_arr) on the stack.
/// `inner(&mut arr)` passes `&mut arr` → FnEntry Unique reborrow of T_arr.
/// Inside inner, `sli.as_mut_ptr()` (even just calling it, discarding the result)
/// fires a Unique reborrow that pops all SRW entries above T_arr — including T_ptr.
/// Back in caller: `*ptr` → T_ptr not in stack → ERROR.
///
/// ## HB behavior
///
/// `arr.as_ptr()` creates T_ptr in HB's exposed_stack (or via Ref path, in reborrow_chain).
/// `inner(&mut arr)` passes a mutable reborrow T_inner. Inside inner,
/// `sli.as_mut_ptr()` creates T_as_mut via another raw retag — pushed to exposed_stack or
/// reborrow_chain as a sibling, NOT removing T_ptr.
/// Back in caller: `*ptr` → T_ptr is still findable in HB's tracking → OK.
///
/// ## TB behavior
///
/// TB uses a tree structure where nodes survive independent sibling reborrows.
/// T_ptr and T_as_mut are sibling nodes; neither's existence invalidates the other.
/// `*ptr` → T_ptr still in tree → pass.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `as_mut_ptr()` Unique reborrow pops SRW(T_ptr) from stack |
/// | TB    | **pass**| T_ptr node survives inner function's reborrow |
/// | HB    | **pass**| T_ptr tag is not removed by a sibling raw reborrow in inner |

#[inline(never)]
fn inner(sli: &mut [i32]) {
    let _ = sli.as_mut_ptr(); // In SB this fires a Unique reborrow, invalidating outer raw ptrs
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut arr = [0i32, 1, 2];
    // Use addr_of! instead of arr.as_ptr(): as_ptr() takes &self (creates a shared
    // reference), and HB's Polonius tracking keeps shared_borrower alive through the
    // next statement, blocking the &mut reborrow. addr_of! creates a raw pointer
    // without going through a reference, so no shared_borrower is set.
    let ptr: *const i32 = core::ptr::addr_of!(arr) as *const i32;

    inner(&mut arr); // SB: as_mut_ptr() inside here invalidates ptr

    let _oof = unsafe { *ptr }; // SB: T_ptr gone; HB: T_ptr still trackable → OK

    0
}
