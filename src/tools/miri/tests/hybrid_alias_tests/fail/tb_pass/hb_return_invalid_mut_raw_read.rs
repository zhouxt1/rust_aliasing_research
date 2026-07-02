#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"). `ret` is never activated (no write before
/// the parent read), so TB keeps it Reserved and tolerates the read. HB's binary `dead` flag
/// kills `ret`'s entry unconditionally on `*xraw`, so the return-site retag now fails the same
/// way it does in the genuinely-activated companion test `hb_tb_return_invalid_mut_write.rs`.
/// Needs the deferred `activated` field to tell these two cases apart.
///
/// A returned `&mut` remains valid after a parent raw READ in HB and TB, but not SB.
///
/// ## Source
/// Ported from the `return_invalid_mut` module of `tree_borrows/pass/sb_fails.rs`.
/// The upstream test fails `stacked_borrows/fail/return_invalid_mut.rs` (SB) but
/// passes `tree_borrows/pass/sb_fails.rs` (TB) and HB.
///
/// ## What this tests
///
/// ```rust
/// fn foo(x: &mut (i32, i32)) -> &mut i32 {
///     let xraw = x as *mut _;
///     let ret = &mut (*xraw).1;
///     let _val = *xraw;    // parent raw READ — SB kills ret; TB/HB do not
///     ret                  // return the child &mut
/// }
/// let ret = foo(&mut (1, 2));
/// // use ret here
/// ```
///
/// In SB, `*xraw` (parent raw read) pops `ret`'s Unique stack item. The return-site
/// retag then fails because `ret`'s tag no longer exists.
///
/// In HB, the base-pointer READ via T_xraw does not truncate the exposed_stack.
/// T_ret is still the top-of-stack entry when `ret` is returned, so the return-site
/// retag succeeds and the caller can use `ret`.
///
/// ## Key difference from `hb_tb_return_invalid_mut_write.rs`
///
/// The TB version (`hb_tb_return_invalid_mut_write.rs`) activates `ret` with a write
/// (`*ret = *ret`) before the parent read. After activation TB's state becomes Active,
/// and a subsequent parent read freezes it — so TB ALSO fails that version.
///
/// This test omits the activation write. Without activation, `ret` stays in
/// Reserved state (TB), which is not frozen by parent reads — so TB passes here.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| `*xraw` pops ret's Unique item; return-site retag fails |
/// | TB    | pass    | ret is Reserved; parent reads do not disable Reserved |
/// | HB    | **fail**| (post-fix regression) base-pointer READ kills ret; return-site retag fails |
#[inline(never)]
fn return_mut_after_parent_read(x: &mut (i32, i32)) -> &mut i32 {
    let xraw = x as *mut (i32, i32);
    let ret = unsafe { &mut (*xraw).1 }; // child &mut via raw ptr (T_ret)
    let _val = unsafe { *xraw }; // parent raw READ: kills ret in SB, harmless in HB/TB
    ret
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut pair = (1i32, 2i32);
    let ret = return_mut_after_parent_read(&mut pair);
    *ret = 42; // write via the returned reference
    0
}
