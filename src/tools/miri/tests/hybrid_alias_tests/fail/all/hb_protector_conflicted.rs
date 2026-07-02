#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/protector_conflicted.rs
// TB: fail — inside the protected closure, `arg` is exposed; a foreign read via ref2 marks arg
// as "conflicted" (child writes become UB while protected); the wildcard write through arg's
// exposed tag is then UB.
// HB: fail — but NOT for the interesting reason. ref1 and ref2 are sequential RawPtr-source
// reborrows of the same `ptr_base`; per the sibling-reborrow-displacement rule, ref2's creation
// already displaces ref1's tag before `protect(ref1)` is ever called. So the FnEntry retag of
// `arg` from `ref1` fails immediately ("invalid parent tag ... no raw pointer stack or current
// borrower match") — the protector-conflict logic this test was designed to exercise is never
// actually reached. Still an overall "fail", matching TB, but this test doesn't tell us anything
// about protector+wildcard interaction specifically; it's the same sibling-reborrow issue seen
// throughout this batch.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: u32 = 42;

    let ptr_base = &mut x as *mut u32;
    let ref1 = unsafe { &mut *ptr_base };
    let ref2 = unsafe { &mut *ptr_base };

    let protect = |arg: &mut u32| {
        let addr = (arg as *mut u32).expose_provenance();
        let wild = core::ptr::with_exposed_provenance_mut::<u32>(addr);

        let _x = *ref2;

        unsafe { *wild = 4 };
    };

    protect(ref1); //~ ERROR: invalid parent tag
    0
}
