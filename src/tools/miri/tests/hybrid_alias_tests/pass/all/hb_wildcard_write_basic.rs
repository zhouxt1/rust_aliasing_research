#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of pass/stacked_borrows/unknown-bottom-gc.rs (wildcard write, basic case).
// SB: pass — expose provenance of a mutable pointer, use wildcard to write through it.
// HB: pass — `ptr`'s tag is both `exposed_tags` (via `expose_provenance`) and the current
//     owner (`current_borrower`) at access time, so the existential wildcard search in
//     `resolve_wildcard_tag` finds it immediately and the write is granted.

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    let mut x = 1u8;
    let ptr = &mut x as *mut u8;
    unsafe {
        // Expose provenance then re-derive a wildcard pointer from the integer address.
        let addr = ptr.expose_provenance();
        let wild = core::ptr::with_exposed_provenance_mut::<u8>(addr);
        // HB: wildcard resolves to ptr's tag (exposed AND current owner) — succeeds.
        *wild = 42;
    }
    0
}
