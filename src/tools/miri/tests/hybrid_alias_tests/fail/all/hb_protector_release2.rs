#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::UnsafeCell;

// Port of fail/tree_borrows/wildcard/protector_release2.rs
// TB: fail — variant of protector_release where ref4 (via wildcard) is created AFTER the
// protected arg3 but BEFORE arg3's exposed child ref5; ref4's descendant (ref6) should still
// get disabled on protector release, since its root predates ref5.
// HB: fail — confirmed, same as hb_protector_release.rs: the final read through `ptr` is denied
// ("invalid parent tag ... Access not allowed"), matching TB's overall verdict.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x: UnsafeCell<[u32; 2]> = UnsafeCell::new([32, 33]);
    let ref1 = &mut x;
    let cell_ptr = ref1.get() as *mut u32;

    let addr = (ref1 as *mut UnsafeCell<[u32; 2]>).expose_provenance();
    let wild = core::ptr::with_exposed_provenance_mut::<UnsafeCell<u32>>(addr);

    let ref2 = unsafe { &mut *cell_ptr };

    let protect = |arg3: &mut u32| {
        // ref4 gets created after the protected ref arg3 but before the exposed ref5.
        let ref4 = unsafe { &mut *wild.wrapping_add(1) };

        *arg3 = 41;

        let ref5 = &mut *arg3;
        let _addr = (ref5 as *mut u32).expose_provenance();

        let ref6 = unsafe { &mut *ref4.get() };

        (ref6 as *mut u32).wrapping_sub(1)

        // Protector release on arg3 happens here.
    };
    let ptr = protect(ref2);
    let _fail = unsafe { *ptr }; //~ ERROR: invalid parent tag
    0
}
