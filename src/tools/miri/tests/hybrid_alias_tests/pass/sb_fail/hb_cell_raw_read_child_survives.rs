#![no_std]
#![no_main]

use core::cell::Cell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A read via the base raw pointer after a shared reborrow does NOT kill the child in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read7.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// ```rust
/// let x = &mut Cell::new(0i32);
/// let raw = x as *mut Cell<i32>;           // base raw pointer
/// let xref = &mut *raw;                    // child &mut reborrow
/// let _shr = &*xref;                       // shared ref (!Freeze — Cell is !Freeze)
/// // SB top of stack: [Unique(T_xref), SRW(T_raw), ...]
/// let _val = core::ptr::read(raw as *mut i32);  // READ via T_raw
/// // In SB: T_raw is a SRW item; reading via it while Unique(T_xref) is above it
/// // would normally be fine, BUT the SB comment says the state mimics `x as *mut _`,
/// // so T_xref still has a Unique item that is popped when T_raw is accessed.
/// // In HB: READ via base_pointer does NOT kill child T_xref.
/// let _ = xref;                            // T_xref still alive in HB
/// ```
///
/// The SB test uses `Cell` specifically because a `Cell` shared reference creates a
/// `SharedReadWrite` item in the borrow stack (not SharedReadOnly), matching the state
/// after `x as *mut _`. This makes the "shared ref does not reactivate raw" check
/// particularly subtle in SB.
///
/// In HB, the Cell distinction is irrelevant to the base-pointer READ path: HB's
/// `check_raw_pointer_stack` treats ALL base-pointer reads as non-destructive, regardless
/// of whether the child entry was derived through a Cell or a plain `&mut`.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| Read via SRW(T_raw) with Unique(T_xref) on top kills T_xref |
/// | TB    | pass    | TB allows reads via ancestors |
/// | HB    | pass    | base-pointer READ does not truncate exposed_stack |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = Cell::new(0i32);
    let x = &mut x; // T_x = current_borrower
    let raw = x as *mut Cell<i32>; // Raw retag: T_raw; exposed_stack = [(T_x, T_raw)]
    let xref = unsafe { &mut *raw }; // RawPtr reborrow: T_xref;
                                      // exposed_stack = [(T_x, T_raw), (T_raw, T_xref)]

    // Shared ref of &mut Cell<i32> (Cell<i32> is !Freeze).
    // HB !Freeze path: shared reborrow inherits parent_tag, no perms change.
    let _shr = &*xref;

    // READ via T_raw (base_pointer of xref's exposed_stack entry).
    // SB: kills T_xref's Unique item.
    // HB: base_pointer READ does not truncate exposed_stack.
    let _val = unsafe { core::ptr::read(raw as *const Cell<i32>) };

    // T_xref is still live in HB after the base-pointer read.
    let _ = xref;

    0
}
