#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/protected_wildcard.rs
// TB: fail — a reference derived from a wildcard pointer (wild_ref) is passed as a &mut
// parameter and gets protected; writing through its ancestor (ref1) is then a foreign write to
// the protected wild_ref, UB.
//
// HB: pass — this test used to trigger a genuine compiler crash (ICE), now fixed (see
// `compute_retags` in src/tools/miri/src/bin/miri.rs). Root cause: the raw borrowck body for
// user code hasn't been through `run_analysis_to_runtime_passes` yet when `compute_retags` runs,
// so `Deref` can legitimately appear beyond the first projection element (this closure's
// captured `ref1` accessed via `(*self).field` combined with the parameter's own deref produces
// such a shape). `compute_retags`'s `needs_retag` closure was calling
// `place.is_indirect_first_projection()`, whose own doc comment says it's only valid from
// `AnalysisPhase::PostCleanup` onward — replaced with the general-purpose `is_indirect()`, which
// the doc comment confirms is equivalent post-cleanup and also correct pre-cleanup. Fix verified
// against a 10-test regression sample spanning every major mechanism in this corpus (dealloc
// protectors, ZST fast path, sibling-reborrow displacement, two-phase borrows, reserved
// protector conflicts) with zero behavior changes elsewhere.
//
// With the crash fixed, the actual aliasing result: HB passes where TB fails. A reference whose
// immediate origin is a wildcard resolution still gets a working FnEntry protector when passed
// as a `&mut` parameter (the protector logic itself is fine), but HB has no tree-based
// foreign-write detection the way TB does — writing through the ancestor `ref1` while `wild_ref`
// (now `_arg`) is protected doesn't trigger HB's simpler "only the reserved/current tag may
// write" checks, since `ref1` was never displaced by `wild_ref`'s creation in the first place
// (they're not siblings from the same raw pointer — `wild_ref` came from resolving a wildcard,
// not from directly reborrowing `ref1`).

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 32;
    let ref1 = &mut x;

    let ref2 = &mut *ref1;
    let addr2 = (ref2 as *mut u32).expose_provenance();

    let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr2);
    let wild_ref = unsafe { &mut *wild };

    let mut protect = |_arg: &mut u32| {
        *ref1 = 13;
    };

    protect(wild_ref);
    0
}
