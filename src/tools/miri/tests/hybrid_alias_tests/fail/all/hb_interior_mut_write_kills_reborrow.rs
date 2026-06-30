#![no_std]
#![no_main]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A write through an `UnsafeCell` raw pointer invalidates a mutable reborrow layered on top.
///
/// ## Source
/// Simplified port of `stacked_borrows/fail/interior_mut1.rs` — that test also fails SB and TB.
///
/// ## What this tests
///
/// ```rust
/// let c = UnsafeCell::new(0i32);
/// let raw = c.get();                    // *mut i32 from UnsafeCell
/// let inner_mut = &mut *raw;            // mutable reborrow via raw ptr; T_inner_mut in exposed_stack
/// let _shr = &*inner_mut;              // shared reborrow → perms = Read
/// unsafe { *raw = 99; }                // WRITE via raw (base pointer) while perms=Read → UB
/// ```
///
/// Setup in HB:
///   1. `c: UnsafeCell<i32>` → allocation A with T_c = initial current_borrower.
///   2. `raw = c.get()` → returns `*mut i32` with tag T_c (no HB retag for UnsafeCell::get).
///   3. `inner_mut = &mut *raw` → RawPtr reborrow: T_inner_mut pushed;
///      exposed_stack = [(T_c, T_inner_mut)].
///   4. `_shr = &*inner_mut` → Shared reborrow (Freeze type): perms → Read,
///      shared_borrower = T_shr.
///   5. `*raw = 99` → WRITE via T_raw (= T_c, base_pointer) while perms = Read.
///      HB dispatches to `Read` branch of `check_borrower_tag`: writes are always denied
///      in Read state → ERROR.
///
/// ## Why SB fails
///
/// In SB, `&mut *raw` pushes Unique(T_inner_mut) above SharedReadWrite(T_raw). `&*inner_mut`
/// pushes SharedReadOnly(T_shr) on top. When `*raw` is written, SB pops T_inner_mut and
/// T_shr (they are above the SRW entry for T_raw).  But first: the write through T_raw
/// requires finding T_raw in the borrow stack; everything above it (including T_shr and
/// T_inner_mut) is popped, making T_shr dead. Any subsequent access through T_shr would fail.
///
/// ## Why this is always UB (all models)
///
/// All three models agree: aliased writes through a parent raw pointer while a child
/// shared borrow is active are undefined behaviour. HB detects it via the Read-state
/// write-deny path; SB detects it by killing the SRO item; TB detects it as a foreign
/// write on a Frozen node.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| write through base ptr kills SRO/Unique items above it |
/// | TB    | **fail**| foreign write on Frozen node is UB |
/// | HB    | **fail**| write in `Read` perms state is always denied |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let c = UnsafeCell::new(0i32);

    let raw = c.get(); // *mut i32 with tag T_c; exposed_stack entry will root here

    // Mutable reborrow via the raw pointer: T_inner_mut pushed to exposed_stack.
    let inner_mut = unsafe { &mut *raw }; //  exposed_stack = [(T_c, T_inner_mut)]

    // Shared reborrow of the i32 (Freeze type): perms → Read, shared_borrower = T_shr.
    let _shr = &*inner_mut;

    // Write via the raw pointer (T_c) while perms = Read.
    // HB: "Access denied: write access on a shared borrow" → UB
    unsafe { *raw = 99 }; //~ ERROR: write access on a shared borrow

    0
}
