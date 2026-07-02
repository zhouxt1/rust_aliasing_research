#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// REGRESSED (2026-07-02): moved here from pass/sb_fail/ after the `RawPointerStack.dead` fix
/// (see COVERAGE.md's "pass/sb_tb_fail case study"). `xref` is never written to before the
/// callee's `&*xraw` shared reborrow (only read, at the very end) — i.e. it is TB-Reserved the
/// whole time, and TB tolerates foreign reads/reborrows on Reserved nodes. HB's binary `dead`
/// flag has no such distinction: the callee's `&*xraw` is treated as a read through the base
/// pointer and kills `xref`'s entry unconditionally, so `*xref` after the call now fails. This
/// is exactly the missing Reserved-vs-Active split tracked as the deferred `activated` field.
///
/// A callee creating a shared reference from the parent raw pointer does NOT invalidate
/// a child `&mut` derived from that same raw pointer in HB.
///
/// ## Source
/// Ported from `stacked_borrows/fail/illegal_read2.rs` — that test **fails** SB but
/// **passes** HB.
///
/// ## What this tests
///
/// In SB:
///   - `xref = &mut *xraw` pushed Unique(T_xref) on top of SRW(T_xraw).
///   - Inside the callee, `shr = &*xraw` creates a SharedReadOnly(T_shr) reborrow from T_xraw.
///     Creating a reborrow from T_xraw (SRW, below Unique) pops Unique(T_xref) first.
///     Stack after callee: [SRO(T_shr), SRW(T_xraw), ...]
///   - When the callee returns and the SRO drops, T_xref is still gone.
///   - `*xref` after the call — ERROR (T_xref not in stack).
///
/// In HB:
///   - `xref = &mut *xraw` → T_xref in exposed_stack: [(T_xraw_base, T_xraw), (T_xraw, T_xref)].
///   - Inside the callee, `&*xraw` creates a shared reborrow with source tag T_xraw.
///     The shared reborrow fires a Ref retag in HB (NewPermission::Read), which overwrites
///     the exposed_stack top's current_borrower to T_shr.  But T_xref is NOT popped — the
///     stack entry structure is preserved.
///   - When the callee returns, the return-borrower machinery restores T_xref as the
///     effective current borrower.
///   - `*xref` succeeds: T_xref is still in the exposed_stack.
///
/// ## Key difference from `sb_raw_read_doesnt_kill_child_mut.rs`
///
/// The callee there reads DIRECTLY via `*xraw` (a raw pointer dereference). Here the callee
/// creates an intermediate shared `&i32` reference from `*xraw` and reads through it — a
/// subtly different reborrow path that SB treats as more destructive (SRO push pops Unique
/// items above the SRW anchor), but HB handles uniformly.
///
/// ## Model verdicts
///
/// | Model | Verdict | Reason |
/// |-------|---------|--------|
/// | SB    | **fail**| SRO reborrow in callee pops T_xref's Unique item |
/// | TB    | pass    | TB preserves children across read-only reborrows (xref is Reserved) |
/// | HB    | **fail**| (post-fix regression) callee's `&*xraw` kills xref's entry unconditionally |
fn callee_creates_shared_ref_from_raw(xraw: *mut i32) {
    // Deliberately uses shared reference (not direct read) to distinguish from illegal_read1.
    let shr = unsafe { &*xraw }; // shared reborrow from the raw pointer
    let _val = *shr;
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 15i32;
    let xraw = &mut x as *mut i32; // T_xraw

    // RawPtr reborrow: T_xref pushed into exposed_stack
    let xref = unsafe { &mut *xraw };

    // Callee creates &*xraw (shared ref) and reads — SB kills T_xref; HB does not
    callee_creates_shared_ref_from_raw(xraw);

    let _val = *xref; // T_xref still in exposed_stack → read succeeds in HB

    0
}
