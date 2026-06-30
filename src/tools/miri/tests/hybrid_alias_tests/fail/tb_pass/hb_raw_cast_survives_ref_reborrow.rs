#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// A raw-pointer cast from `&mut T` is invalidated by a Ref reborrow of the same place.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_write2.rs`.
/// That test fails SB. This test also fails HB — but Tree Borrows passes it.
///
/// ## What this tests
///
/// ```rust
/// let target = &mut 42;
/// let target2 = target as *mut i32; // T_target2 derived from T_target (Raw retag)
/// {
///     let _rb = &mut *target; // short-lived Ref reborrow from target
/// }
/// *target2 = 13;  // SB: T_target2 was popped; HB: T_target2 was overwritten in exposed_stack
/// ```
///
/// In SB:
///   - `target2 = target as *mut _` pushes SRW(T_target2) ABOVE Unique(T_target).
///   - `&mut *target` (Unique reborrow from T_target) pops SRW(T_target2) off the stack.
///   - `*target2 = 13` — T_target2 not in stack → ERROR.
///
/// In HB:
///   - `target2 = target as *mut _` → Raw retag: exposed_stack = [(T_target, T_target2)].
///   - `&mut *target` → Ref reborrow: `apply_reborrow_to_stack(Ref, ...)` overwrites the
///     exposed_stack top's `current_borrower` to T_reborrow — T_target2 is lost!
///   - When `_rb` is dropped and the PoloniusAnchor fires, it restores T_target as current,
///     but T_target2 remains lost (it was the overwritten stack entry, not the `current_borrower`
///     field). `*target2 = 13` via T_target2 → "neither current nor prev" → ERROR.
///
/// In Tree Borrows:
///   - T_target2 is a node in the tree (child of T_target). The Ref reborrow creates T_rb as a
///     sibling node. T_rb going out of scope does not remove T_target2 from the tree.
///   - `*target2 = 13` writes via T_target2 (a raw-pointer Active/Reserved node in TB) → pass.
///
/// This test fills the previously empty `fail/tb_pass/` slot: SB and HB both detect UB here,
/// but Tree Borrows does not.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| T_target2 (SRW) popped when Unique reborrow `&mut *target` is created |
/// | TB    | **pass**| T_target2 node survives independent sibling reborrows in the tree |
/// | HB    | **fail**| Ref reborrow overwrites T_target2 in exposed_stack; T_target2 lost |
#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let target = &mut 42i32; // T_target = current_borrower
    let target2 = target as *mut i32; // Raw retag: T_target2 derived from T_target

    // Short-lived Ref reborrow of *target — in SB this pops T_target2 from the stack.
    // In HB the reborrow is tracked via the exposed_stack and restored on drop.
    let _rb = &mut *target;
    drop(_rb);

    unsafe { *target2 = 13 }; //~ ERROR: T_target2 overwritten in HB exposed_stack; gone in SB

    0
}
