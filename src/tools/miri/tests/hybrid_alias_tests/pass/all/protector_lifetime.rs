#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Protector lifetime test: the StrongProtector installed at function entry must be
/// fully released when the function returns, so that aliasing accesses that would
/// have been UB *during* the call are valid *after* it.
///
/// ## The scenario
///
/// 1. `z` is derived from a raw pointer `raw` that aliases `data`.
/// 2. `just_write(z)` is called — FnEntry retag gives `x` a new tag `T_safe` with a
///    StrongProtector. During the call, any write through `raw` (T_y base pointer) would
///    be UB because `T_safe` is protected.
/// 3. After `just_write` returns, `on_stack_pop` fires:
///    `release_protector(T_safe)` checks the implicit read succeeds, then
///    `end_call` removes `T_safe` from `global.protected_tags`.
/// 4. `*raw = 99` is now a plain write through the raw alias — no protector is
///    active, so it succeeds.
///
/// If the protector were NOT released at frame exit, step 4 would fire UB
/// (base_pointer match in check_raw_pointer_stack would find T_safe still protected).
/// This test therefore confirms the full Phase 3 lifecycle:
///   install protector (Phase 1) → enforce during call (Phase 2) → release at return (Phase 3).
fn just_write(x: &mut i32) {
    *x = 42;
    // Frame exits here → on_stack_pop → release_protector(T_safe) → end_call removes T_safe
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut data = 0i32;

    // Build the raw pointer alias chain (same pattern as test6):
    //   data → y (T_y) → raw (carries T_y) → z = &mut *raw (T_z in exposed_stack)
    let y = &mut data;
    let raw = y as *mut i32;

    unsafe {
        let z = &mut *raw;

        // During just_write(z): T_safe is StrongProtector for z's allocation.
        // A write through `raw` here would be UB (T_safe is active).
        just_write(z);

        // After just_write returns: T_safe has been released by on_stack_pop.
        // This write goes through the raw base pointer (T_y).
        // check_raw_pointer_stack finds no protector for the stack entry → OK.
        *raw = 99;
    }

    // data == 99; the write through raw succeeded because the protector was gone.
    0
}
