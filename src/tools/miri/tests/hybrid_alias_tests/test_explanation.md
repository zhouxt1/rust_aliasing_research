# Hybrid Borrows test corpus

Each `.rs` file in [pass/](pass/) and [fail/](fail/) is a standalone test program using
`#![no_std]` / `#![no_main]` with a `miri_start` entry point. They use only `core`, not
`std` — no heap allocation (no `Box`, `Vec`, `String`).

- **`pass/`** cases must finish under `-Zmiri-hybrid-borrows` with no aliasing error.
- **`fail/`** cases must report an aliasing violation.

Tests are grouped by origin. The prefix `sb_` means ported from a Stacked Borrows test;
`tb_` means ported from a Tree Borrows test; unprefixed tests are HB-original.

---

## Key HB behavioural properties (referenced throughout)

| Property | HB | SB | TB |
|----------|----|----|-----|
| Parent **write** kills child borrow | ✅ | ✅ | ✅ (freezes) |
| Parent **read** kills/freezes child borrow | ❌ no effect | ✅ kills Unique | ✅ freezes Reserved/Active |
| FnEntry retag invalidates raw pointer | ❌ (borrower restored on return) | ✅ (Unique popped) | ❌ |
| Sequential `&mut` reborrows: second kills first | ✅ | ✅ | ✅ |
| `StrongProtector` blocks aliased accesses (read+write) | ✅ | ✅ | ✅ |
| Foreign write during `Reserved` 2-phase borrow (`Freeze`) | ❌ (UB) | ✅ (ok) | ❌ (UB) |
| Foreign write during `ReservedIM` 2-phase borrow (`!Freeze`) | ✅ (ok) | ✅ | ✅ |

---

## Pass cases

### [pass/raw_ptr_from_mut_parent_restore.rs](pass/raw_ptr_from_mut_parent_restore.rs)
**Raw pointer from `&mut`, parent access restoration**

```rust
let ref1 = &mut x;
let ref2 = &mut *ref1;
let ptr2 = ref2 as *mut i32;
unsafe { *ptr2 += 1; }   // write via raw (= ref2's tag)
*ref2 += 1;              // write via ref2
unsafe { *ptr2 += 1; }   // raw again
*ref1 += 1;              // ref2/ptr2 die, ref1 takes over
x += 1;                  // ref1 dies, x regains access
```

Exercises the exposed-stack unwinding as the reborrow chain unwinds back to `ref1` then
`x`. The key invariant is that interleaving `*ptr2` / `*ref2` writes does not invalidate
`ref2`, and that `ref1` / `x` can reclaim the borrow once their descendants die.

---

### [pass/shared_reborrow_chains.rs](pass/shared_reborrow_chains.rs)
**Shared-reborrow chains, conditional origins, `&mut` reassignment**

Bundles five sub-tests (miri_start calls test4):
- `test1` — trivial `&x` baseline.
- `test2` — chain `ref1 → sref1 → sref2` of shared reborrows from a `&mut`.
- `test3` — sibling shared reborrows of the same `&mut`; still flaky.
- `test4` — mutable reference reassigned while old shared reborrows are live.
- `test5` — reference with two possible origins (if/else).

---

### [pass/fn_call_retag_disjoint_args.rs](pass/fn_call_retag_disjoint_args.rs)
**Function-call retag boundary and disjoint mutable arguments**

Tests:
- `test1/test2`: Pass `&mut x` to a callee that reborrows it; caller regains access after.
- `test3/test4`: Two disjoint `&mut` arguments (`&mut x`, `&mut y`) to the same callee.

Exercises the return-borrower restoration when a callee's frame is popped, and confirms
that disjoint mutable arguments can coexist.

---

### [pass/reborrow_in_loop.rs](pass/reborrow_in_loop.rs)
**Reborrow created and released inside each loop iteration**

```rust
while i < 3 {
    let ref2 = &mut x;
    *ref2 += 1;
}
```

Validates that a fresh `&mut x` inside each iteration is correctly retagged and released,
and that Polonius facts at the back-edge leave no stale borrower.

---

### [pass/two_phase_method_call.rs](pass/two_phase_method_call.rs)
**Two-phase borrow via nested method call**

```rust
board.add_score(board.get_current_score());
```

Classic two-phase pattern: argument evaluation (`get_current_score`) takes a shared borrow
while the outer call (`add_score`) reserves a mutable borrow. Tests the
`Reserved → Write` activation path.

---

### [pass/mut_ref_from_raw_ptr.rs](pass/mut_ref_from_raw_ptr.rs)
**`&mut` created from a raw pointer (symmetric to raw_ptr_from_mut_parent_restore)**

```rust
let ref1 = &mut x;
let raw1 = ref1 as *mut i32;
let ref2 = unsafe { &mut *raw1 };  // RawPtr reborrow: ref2 in exposed stack
*ref2 += 1;
*ref1 += 1;  // ref2 dies, ref1 takes over
```

The new `&mut` is born from a raw pointer rather than a reference. Validates that
`RawPtr`-source reborrows register in the exposed stack so parent access restoration
works.

---

### [pass/core_lib_integration.rs](pass/core_lib_integration.rs)
**Core library API integration smoke tests**

Tests 11 `core` APIs that internally rely on reborrows the tracker must handle:
`mem::replace`, `mem::swap`, `mem::take`, `Option::as_mut`, `Option::get_or_insert`,
`Option::insert`, `Option::replace`, `slice::first_mut`, `slice::split_at_mut`,
`slice::iter_mut`, `slice::swap`, `slice::split_first_mut`.

`miri_start` currently invokes only `test_option_insert()`. Inline comments mark which
subtests pass today; commented-out calls form a TODO list for future coverage.

---

### [pass/protector_lifetime.rs](pass/protector_lifetime.rs)
**`StrongProtector` is fully released when the function returns**

Confirms the full Phase 3 lifecycle: install protector at FnEntry → enforce during call
→ release at return. After `just_write(z)` returns, a write through the raw alias
`raw` succeeds because the protector is no longer active.

---

### [pass/unsafecell_shared_write.rs](pass/unsafecell_shared_write.rs) *(Phase 1 — currently fails)*
**`UnsafeCell<i32>` write via shared reference**

`UnsafeCell<i32>` is `!Freeze`. Under Phase 1 tag inheritance, `&c` returns
`parent_tag` (no new `shared_borrower`, no `Read` transition). Writes via `.get()`
go through the normal `Write` check and succeed.

**Status:** fails until Phase 1 is implemented. Contrast with `fail/freeze_shared_raw_write.rs`
which does the same with plain `&i32` and must keep failing.

---

### [pass/unsafecell_aliasing_writes.rs](pass/unsafecell_aliasing_writes.rs) *(Phase 1 — currently fails)*
**Two aliasing `&UnsafeCell<i32>` references, both writing**

Canonical interior-mutability aliasing pattern. Under Phase 1, both `&c` reborrows
return `parent_tag`; `r1` and `r2` carry the same tag and writes through either succeed.

**Status:** fails until Phase 1.

---

### [pass/cell_ptr_write.rs](pass/cell_ptr_write.rs) *(Phase 1 — currently fails)*
**`Cell<i32>` writes via `.as_ptr()`**

Same pattern as `unsafecell_aliasing_writes` but through the `Cell<T>` abstraction.
`Cell::as_ptr()` is an inline pointer cast requiring no complex Polonius MIR.

**Status:** fails until Phase 1.

---

### [pass/two_phase_not_freeze_reservedim.rs](pass/two_phase_not_freeze_reservedim.rs)
**Two-phase borrow over `!Freeze` type (`ReservedIM`)**

`Cell<i32>` is `!Freeze`. A two-phase `&mut Cell<i32>` reservation must tolerate a
write through the shared alias `r` during the reservation window, because `Cell` may
be mutated via `&Cell<i32>`. Tests `ReservedIM::Write` acceptance of the
`shared_borrower` tag.

---

### [pass/not_freeze_arg_no_protector.rs](pass/not_freeze_arg_no_protector.rs)
**`!Freeze` shared ref function argument must NOT receive a protector (Phase 3)**

When `&T where T: !Freeze` is passed as a function argument, Phase 3 suppresses the
protector. A write via a raw alias inside the callee succeeds. Contrast with
`fail/protected_shared_foreign_write.rs` which uses `&i32` (Freeze) and must fail.

---

### [pass/sb_raw_read_doesnt_kill_child_mut.rs](pass/sb_raw_read_doesnt_kill_child_mut.rs)
**Parent raw READ does not kill a child `&mut` — HB more permissive than SB**

Source: `stacked_borrows/fail/illegal_read1.rs` (fails SB, passes HB and TB).

A callee reads via `xraw` (the base_pointer of `xref`'s exposed-stack entry). In HB,
reads via the base_pointer do not kill the child `current_borrower`. `xref` remains
valid after the callee returns.

SB fails this because any use of a Unique tag (read or write) pops items above it.

---

### [pass/sb_fnentry_doesnt_invalidate_raw.rs](pass/sb_fnentry_doesnt_invalidate_raw.rs)
**FnEntry retag does not invalidate a previously created raw pointer — HB like TB**

Source: `stacked_borrows/fail/fnentry_invalidation.rs` (fails SB, passes HB and TB).

`x.do_bad()` FnEntry-retagged `&mut self` does not permanently displace `z` (a raw
pointer created before the call). After `do_bad` returns with no write, HB's
return-borrower machinery restores `current_borrower = T_z`.

---

### [pass/sb_return_reserved_ref_after_parent_read.rs](pass/sb_return_reserved_ref_after_parent_read.rs)
**Returning a `&mut` whose parent was only read is valid — HB more permissive than SB**

Source: `stacked_borrows/fail/return_invalid_mut.rs` (fails SB, passes HB).

`*xraw` (READ via base_pointer) does not kill `ret` (child RawPtr reborrow) in HB.
`ret` is still `current_borrower` at the return site; the return reborrow succeeds.

---

### [pass/sb_pass_reserved_ref_after_parent_read.rs](pass/sb_pass_reserved_ref_after_parent_read.rs)
**Passing a `&mut` whose parent was only read is valid — HB more permissive than SB**

Source: `stacked_borrows/fail/pass_invalid_mut.rs` (fails SB, passes HB).

Same as the return test above, but the invalidated-in-SB reference is passed as a
function argument rather than returned.

---

### [pass/tb_alternate_rw_parent_read_no_freeze.rs](pass/tb_alternate_rw_parent_read_no_freeze.rs)
**Alternating parent reads + child writes — parent reads do not freeze in HB**

Source: `tree_borrows/fail/alternate-read-write.rs` (fails TB, passes HB).

TB fails because a Foreign Read causes child `Reserved/Active` to transition to `Frozen`,
making subsequent writes UB. HB does not implement this freeze rule: parent reads have no
effect on the child's borrower state.

---

### [pass/tb_parent_read_no_freeze_raw.rs](pass/tb_parent_read_no_freeze_raw.rs)
**Parent read does NOT kill a raw pointer in HB**

Source: `tree_borrows/fail/parent_read_freezes_raw_mut.rs` (fails TB, passes HB).

In TB, a parent read on `root` transitions `ptr`'s Unique node to Frozen; the second
write via `ptr` then fails. In HB, the parent read has no effect: `ptr` (carrying the
same tag as `mref`) remains `current_borrower` and the second write succeeds.

---

## Fail cases

### [fail/raw_ptr_invalid_after_parent_reclaim.rs](fail/raw_ptr_invalid_after_parent_reclaim.rs)
**Raw pointer invalid after parent performs a write**

Diff from `pass/raw_ptr_from_mut_parent_restore.rs`: one extra `unsafe { *ptr2 += 1; }`
at the end. `*ref1 += 1` (parent write) kills `ref2`/`ptr2`; the subsequent raw access
is UB.

---

### [fail/raw_write_during_shared_borrow.rs](fail/raw_write_during_shared_borrow.rs)
**Writing via raw pointer while a shared reborrow is live**

```rust
let raw2 = ref1 as *mut i32;
let sref1 = &*ref1;            // shared reborrow → Read/Frozen state
unsafe { *raw2 += 1; }         // write via raw — allocation is in Read, write denied
```

Tests shared-vs-raw conflict detection. HB detects the violation at the write site
(unlike SB/TB which detect it at the subsequent shared read).

---

### [fail/sibling_mut_from_same_raw.rs](fail/sibling_mut_from_same_raw.rs)
**Two `&mut` from the same raw pointer are mutually exclusive**

```rust
let ref2 = unsafe { &mut *raw1 };
let ref3 = unsafe { &mut *raw1 };  // creating ref3 kills ref2
*ref2 += 1;                        // UB: ref2 is dead
```

Creating `ref3` overwrites `ref2`'s entry in the exposed stack. The subsequent write
via `ref2` fails.

---

### [fail/protected_shared_foreign_write.rs](fail/protected_shared_foreign_write.rs)
**Protected `&i32` argument — raw write through alias is UB**

```rust
fn optimize_me(safe_ref: &i32, raw_ptr: *mut i32) {
    let _ = *safe_ref;
    unsafe { *raw_ptr = 42; }  // UB: raw_ptr aliases safe_ref, which has StrongProtector
    let _ = *safe_ref;
}
```

`safe_ref` is a `Freeze` type → receives a `StrongProtector` at FnEntry. Any access via
the aliased `raw_ptr` during the call is denied.

---

### [fail/protected_mut_foreign_write.rs](fail/protected_mut_foreign_write.rs)
**Protected `&mut i32` argument — raw write through alias is UB**

Same as `protected_shared_foreign_write`, but with `&mut i32` as the protected argument.

---

### [fail/protected_mut_raw_access_uncaught.rs](fail/protected_mut_raw_access_uncaught.rs)
**Protected `&mut` with raw alias — UB at raw access site (current system may not catch)**

Similar to `protected_mut_foreign_write`, but the body only reads `safe_ref` once, so
HB detects the violation at `*raw_ptr` itself. The comment in the file notes the system's
current coverage limit for this pattern.

---

### [fail/freeze_shared_raw_write.rs](fail/freeze_shared_raw_write.rs)
**`Freeze` shared reference + raw write — UB detected at the write site**

```rust
let frozen: &i32 = unsafe { &*raw };   // Freeze type → BorrowerPermission::Read
unsafe { *raw = 42; }                  // WRITE while in Read state → UB in HB
let _ = *frozen;
```

Counterpart to `pass/unsafecell_shared_write.rs`. `i32` is `Freeze` → `Read` permission
→ writes unconditionally denied. HB catches the UB at the write site (SB/TB only catch
it at the subsequent read). Must continue failing after Phase 1.

---

### [fail/sb_sequential_reborrows_kill_first.rs](fail/sb_sequential_reborrows_kill_first.rs)
**Second `&mut` reborrow kills the first raw pointer**

Source: `stacked_borrows/fail/raw_tracking.rs`.

```rust
let raw1 = &mut l as *mut i32;  // T1 = current_borrower
let raw2 = &mut l as *mut i32;  // T2 = new current_borrower, T1 displaced
unsafe { *raw1 = 13 };          // UB: T1 no longer current_borrower
```

Unlike the parent-read tests (which pass HB), a new `&mut` reborrow is a write-like
operation that displaces the previous borrower. This matches SB and TB behaviour.

---

### [fail/sb_raw_read_violates_mut_protector.rs](fail/sb_raw_read_violates_mut_protector.rs)
**Aliased raw READ during a protected `&mut` argument is UB**

Source: `stacked_borrows/fail/invalidate_against_protector1.rs`.

```rust
fn inner(x: *mut i32, _y: &mut i32) {
    let _val = unsafe { *x };  // read via aliased raw — violates StrongProtector on _y
}
```

Even a READ through an aliased pointer violates the `StrongProtector` because `&mut`
guarantees exclusive access for the entire duration of the call (both read and write).

Contrast with `pass/sb_raw_read_doesnt_kill_child_mut.rs` where the read is via the
base_pointer WITHOUT a protector — that is valid.

---

### [fail/tb_write_during_two_phase_reservation.rs](fail/tb_write_during_two_phase_reservation.rs)
**Foreign write during a `Reserved` 2-phase borrow (Freeze type) is UB**

Source: `tree_borrows/fail/write-during-2phase.rs`.

```rust
let alias = &mut f.0 as *mut u64;  // u64 is Freeze
let _res = f.add(unsafe {
    *alias = 42;  // UB: foreign write during Reserved state (Freeze type)
    0
});
```

`u64` is `Freeze` → 2-phase creates `Reserved` (not `ReservedIM`). `Reserved` does NOT
tolerate foreign writes; the write via `alias` during the reservation is denied.

SB passes this (tolerant of foreign writes during 2-phase). TB fails. HB fails.

Contrast with `pass/two_phase_not_freeze_reservedim.rs` where `Cell<i32>` (`!Freeze`)
creates `ReservedIM`, which tolerates foreign writes.

---

## Phase status summary

| Phase | What it enables | Tests gated on it |
|-------|----------------|-------------------|
| Phase 1 | `!Freeze` shared refs inherit `parent_tag` (no `Read` transition) | `pass/unsafecell_shared_write`, `pass/unsafecell_aliasing_writes`, `pass/cell_ptr_write` |
| Phase 2 | `!Freeze` 2-phase borrows use `ReservedIM` | `pass/two_phase_not_freeze_reservedim` |
| Phase 3 | `!Freeze` shared-ref fn args suppress `StrongProtector` | `pass/not_freeze_arg_no_protector` |

## Known gaps

- GC visits only `current_borrower`; `shared_borrower`, `prev_borrower`, and raw-stack
  entries are not reported, which may suppress liveness errors.
- `ReturnBorrowers` correctness issue: some shared-loan return paths have a known bug
  (see inline comment in `pass/shared_reborrow_chains.rs :: test3`).
- Non-concrete tag accesses (e.g. pointer passed as `*const ()` and cast back) are
  ignored.
- `hb_protect_place`, `hb_expose_tag`, `hb_give_pointer_debug_name`, and
  `hb_print_borrow_state` are not implemented.
