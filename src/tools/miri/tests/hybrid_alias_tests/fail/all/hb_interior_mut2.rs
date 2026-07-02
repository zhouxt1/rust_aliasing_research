#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

use core::cell::UnsafeCell;
use core::mem;

// Port of fail/stacked_borrows/interior_mut2.rs
// SB: fail — writing through a raw pointer from an outer UnsafeCell invalidates an inner
// shared reference derived via mutable_transmutes.
// HB: fail — confirmed. Overwriting `*c.get()` with a fresh `UnsafeCell::new(0)` invalidates
// `inner_shr`'s parent tag lineage; the final `*inner_shr.get()` reborrow is denied
// ("invalid parent tag ... no raw pointer stack or current borrower match").

#[allow(mutable_transmutes)]
unsafe fn unsafe_cell_get<T>(x: &UnsafeCell<T>) -> &'static mut T {
    mem::transmute(x)
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    unsafe {
        let c = &UnsafeCell::new(UnsafeCell::new(0));
        let inner_uniq = &mut *c.get();
        let inner_shr = &*inner_uniq;

        let _val = c.get().read();

        let _val = *unsafe_cell_get(inner_shr);

        *c.get() = UnsafeCell::new(0);

        let _val = *inner_shr.get();
    }
    0
}
