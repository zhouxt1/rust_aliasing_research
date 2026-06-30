#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A raw pointer "smuggled" into a global is invalid after the callee's borrow is restored.
///
/// ## Source
/// Ported from `stacked_borrows/fail/pointer_smuggling.rs` (no imports needed — static
/// is declared directly; in no_std context this is fine).
///
/// ## What this tests
///
/// ```
/// fn fun1(x: &mut u8) { unsafe { PTR = x; } }  // stores FnEntry-retagged tag in global
///
/// let val = &mut val;             // T_val = current_borrower
/// fun1(val);                      // FnEntry retag: T_fun_x = new current, T_val = prev
///                                 // PTR stores T_fun_x; on return, T_val restored as current
/// *val = 2;                       // write via T_val (current_borrower) — OK
/// fun2();                         // reads *PTR (carries T_fun_x) — T_fun_x is dead → UB
/// ```
///
/// `fun1` stores the FnEntry-retagged raw pointer (T_fun_x) into a global `PTR`. When
/// `fun1` returns, HB's return-borrower machinery restores T_val as current_borrower.
/// T_fun_x is no longer active. Any subsequent access via PTR (T_fun_x) is UB.
///
/// The write `*val = 2` then confirms T_val is the current_borrower. `fun2` reads via
/// T_fun_x (dead) → access fails.
///
/// ## HB vs SB
///
/// Both models fail this. SB: the FnEntry Unique T_fun_x is popped when T_val is used
/// again after the call. HB: T_fun_x was displaced from current_borrower when fun1 returned;
/// T_val is restored; accessing via T_fun_x fails the check.
static mut PTR: *mut u8 = 0 as *mut u8;

#[inline(never)]
fn fun1(x: &mut u8) {
    unsafe { PTR = x as *mut u8; } // store FnEntry-retagged pointer in global
}

#[inline(never)]
fn fun2() {
    // PTR carries T_fun_x from the FnEntry retag inside fun1.
    // After fun1 returned, T_fun_x is dead (T_val was restored as current_borrower).
    let _x = unsafe { *PTR }; //~ ERROR: T_fun_x no longer current_borrower
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut val = 0u8;
    let val_ref = &mut val; // T_val = current_borrower of val's allocation

    fun1(val_ref); // stores FnEntry T_fun_x in PTR; returns → T_val restored

    *val_ref = 2; // WRITE via T_val (current_borrower) — invalidates T_fun_x (it was prev)

    fun2(); // reads *PTR (T_fun_x) → UB

    0
}
