#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of fail/stacked_borrows/static_memory_modification.rs
// SB: fail — "writing to ALLOC which is read-only". SB's retag needs mutable access to the
// allocation to register the new tag, which fails at the base-interpreter level
// (`throw_ub!(WriteToReadOnly(id))` in rustc_const_eval's memory.rs) since `X`'s
// `Allocation::mutability` is `Mutability::Not`.
// HB: fail, but via a different mechanism — HB's own retag logic rejects creating a non-shared
// (`&mut`) reference from a tag that is currently in shared-borrow state ("Attempting to create
// a non-shared reference using a shared tag") before it ever reaches the point of needing
// mutable allocation access. So the base-interpreter read-only check is never even exercised
// here; HB's existing shared/mutable conflict detection independently produces the same UB
// verdict. No HB-specific "static memory" feature was needed either way.

static X: usize = 5;

#[no_mangle]
#[allow(mutable_transmutes)]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let _x = unsafe { core::mem::transmute::<&usize, &mut usize>(&X) }; //~ ERROR: non-shared reference using a shared tag
    0
}
