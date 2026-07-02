#![no_std]
#![no_main]

use core::panic::PanicInfo;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

// Port of pass/stacked_borrows/stacked-borrows.rs
// SB: pass — both sub-patterns are accepted by SB (the upstream file notes "these do not work
// the same under TB"); the original also carries a FIXME about miscompilation under
// optimizations (mitigated upstream with -Zmir-opt-level=0, not replicated here since our
// harness does not apply that optimization pipeline the same way).
// TB: fail on `two_phase_aliasing_violation` — "since we treat 2phase as raw, we [SB] do
// accept it. Tree Borrows rejects it."
// HB: fail — confirmed, in `two_phase_aliasing_violation`: writing through `alias` (`*alias =
// 42`) while `self` is a Reserved two-phase borrow is a foreign write via a non-reserving tag,
// which HB denies ("only the reserved tag may activate"). `mut_raw_mut2` alone passes; the
// failure is specifically the two-phase pattern. This is a genuine 3-way divergence (SB pass,
// TB fail, HB fail) — HB sides with TB here, the same pattern as `hb_cell_protected_write.rs`.

fn mut_raw_mut2() {
    unsafe {
        let mut root = 0;
        let to = &mut root as *mut i32;
        *to = 0;
        let _val = root;
        *to = 0;
    }
}

fn two_phase_aliasing_violation() {
    struct Foo(u64);
    impl Foo {
        fn add(&mut self, n: u64) -> u64 {
            self.0 + n
        }
    }

    let mut f = Foo(0);
    let alias = &mut f.0 as *mut u64;
    let res = f.add(unsafe {
        *alias = 42;
        0
    });
    assert_eq!(res, 42);
}

#[no_mangle]
pub fn miri_start(_argc: isize, _argv: *const *const u8) -> isize {
    mut_raw_mut2();
    two_phase_aliasing_violation();
    0
}
