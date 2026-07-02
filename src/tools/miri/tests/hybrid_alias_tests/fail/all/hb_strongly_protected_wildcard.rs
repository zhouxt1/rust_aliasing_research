#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/tree_borrows/wildcard/strongly_protected_wildcard.rs
// TB: fail — `f` takes a raw pointer (as usize) so the only protector active is the one on
// `x`; deallocating through it while `x`'s protector is active is UB.
// HB: fail — confirmed, cleanly. `before_memory_deallocation` ignores its `_prov_extra`
// parameter entirely and just scans every tag in `BorrowerState` for a `StrongProtector`,
// regardless of how the deallocating pointer was derived — so it doesn't matter that the address
// arrived as a plain `usize` rather than a concrete tag. `x`'s FnEntry protector is found and
// the deallocation is correctly denied. Uses miri_alloc/miri_dealloc instead of Box (Box is
// broken under this fork — see project memory on the Polonius-MIR-for-stdlib bug).

extern "Rust" {
    fn miri_alloc(size: usize, align: usize) -> *mut u8;
    fn miri_dealloc(ptr: *mut u8, size: usize, align: usize);
}

#[inline(never)]
fn inner(x: &mut i32, f: fn(usize)) {
    // `f` may mutate, but it may not deallocate! `f` takes a raw address so that the only
    // protector is that on `x`.
    f(x as *mut i32 as usize)
}

fn dealloc_via_addr(addr: usize) {
    unsafe { miri_dealloc(addr as *mut u8, 4, 4) }; //~ ERROR: strongly protected
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let ptr = miri_alloc(4, 4) as *mut i32;
        core::ptr::write(ptr, 0);
        inner(&mut *ptr, dealloc_via_addr);
    }
    0
}
