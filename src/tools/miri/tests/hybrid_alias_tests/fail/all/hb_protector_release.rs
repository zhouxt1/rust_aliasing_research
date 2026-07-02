#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::UnsafeCell;

// Port of fail/tree_borrows/wildcard/protector_release.rs
// TB: fail — checks that releasing a protector correctly determines certain tags cannot be
// its children (ref3's wildcard root has a smaller tag than the protected tag's exposed child),
// so ref3 should get disabled on protector release; the final read through the returned pointer
// (derived from ref3) is then UB.
// HB: fail — confirmed, matching TB's overall verdict (though not via the same tree-descendant
// reasoning): the final read through `ptr` is denied ("invalid parent tag ... Access not
// allowed"). HB's protector+wildcard interaction produces a sensible UB result here even without
// any tree-based ancestor/descendant computation.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: UnsafeCell<[u32; 2]> = UnsafeCell::new([32, 33]);
    let ref1 = &mut x;
    let cell_ptr = ref1.get() as *mut u32;

    let addr = (ref1 as *mut UnsafeCell<[u32; 2]>).expose_provenance();
    let wild = core::ptr::with_exposed_provenance_mut::<UnsafeCell<u32>>(addr);

    let ref2 = unsafe { &mut *cell_ptr };

    // ref3 gets created before the protected ref `arg4`.
    let ref3 = unsafe { &mut *wild.wrapping_add(1) };

    let protect = |arg4: &mut u32| {
        // Activates arg4. This would disable ref3 at [0] if it wasn't a cell.
        *arg4 = 41;

        // Creates an exposed child of arg4.
        let ref5 = &mut *arg4;
        let _addr = (ref5 as *mut u32).expose_provenance();

        // This creates ref6 from ref3 at [1], so that it doesn't disable arg4 at [0].
        let ref6 = unsafe { &mut *ref3.get() };

        // Creates a pointer to [0] with the provenance of ref6.
        (ref6 as *mut u32).wrapping_sub(1)

        // Protector release on arg4 happens here.
    };
    let ptr = protect(ref2);
    // ref6 is disabled at [0] under TB (ref3's root predates arg4's exposed child).
    let _fail = unsafe { *ptr }; //~ ERROR: invalid parent tag
    0
}
