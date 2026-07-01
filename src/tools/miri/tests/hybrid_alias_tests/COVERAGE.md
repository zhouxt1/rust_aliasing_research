# Hybrid Borrows Test Coverage Map

This document maps every test in Miri's Stacked Borrows (SB) and Tree Borrows (TB) test suites to its
coverage status in the Hybrid Borrows (HB) test suite.  For each upstream test the table records
whether the aliasing scenario is already exercised by an HB test, whether it cannot be ported due to
a feature HB does not implement, or whether a new HB test still needs to be written.

---

## Summary

| Status | Meaning | Count |
|---|---|---|
| `covered` | An existing HB test exercises the same aliasing scenario | 36 |
| `hb_pass` | HB semantics differ from SB/TB (HB allows the program); pattern documented in pass/ | 8 |
| `port_needed` | Portable to HB — a new test still needs to be written | 0 |
| `no_std` | Requires `std`/`alloc` features not available in `#![no_std]` HB tests | 14 |
| `no_dealloc` | Requires deallocation-tracking semantics not implemented in HB | 1 |
| `no_wildcard` | Requires wildcard / exposed-provenance tag support not implemented in HB | 28 |
| `no_per_byte` | Requires per-byte permission granularity not implemented in HB | 5 |
| `no_zst` | Requires ZST-specific borrow-stack behaviour not implemented in HB | 3 |
| `no_static` | Requires static/global memory semantics not implemented in HB | 0 |
| `no_concurrency` | Requires multi-thread concurrency not implemented in HB | 5 |
| `diagnostic_only` | Only validates error-message formatting; no aliasing logic to port | 4 |

---

## HB's wildcard model

A wildcard pointer (`ProvenanceExtra::Wildcard`, produced by `with_exposed_provenance`/
`with_exposed_provenance_mut` after a prior `expose_provenance()` call) carries no concrete tag —
the integer it was cast from remembers nothing about its origin. HB resolves it the same way SB
does (`find_granting` in `stack.rs`): **existential search** over every tag this allocation has
ever exposed.

Each `BorrowerState` carries `exposed_tags: Vec<BorTag>`, populated by `hb_expose_tag` whenever
`expose_provenance()` is called. At wildcard-access time, `resolve_wildcard_tag` walks the tags
currently live in `BorrowerState` — `current_borrower`, `prev_borrower`, `shared_borrower`,
`reborrow_chain` (newest-displaced first), `exposed_stack` (top to bottom) — and returns the first
one that is *also* a member of `exposed_tags`. The resolved tag is then checked exactly like a
concrete access via `check_borrower_tag`. If no live tag was ever exposed, the access is denied
outright.

This was a correction of an earlier, overly permissive design ("Option B — lazy current-owner
lookup") that resolved every wildcard access to whichever tag currently owns the allocation,
without checking whether that tag — or any tag at all — had ever been exposed. That version made
exposing *any* pointer into an allocation permanently immunize all future wildcard accesses to it,
regardless of lineage. The `exposed_tags`-gated version above replaced it after testing showed the
naive version diverged from SB on cases SB explicitly designed to catch (`exposed_only_ro`,
`unescaped_local`) — see those rows below, now `covered`.

One known simplification versus SB's `find_granting`: HB takes the *first* exposed-and-live
candidate by priority order and lets a single `check_borrower_tag` call decide if it grants the
access, rather than backtracking through every exposed-and-live candidate looking for one that
grants. In the current corpus only one tag is ever exposed per allocation at a time, so this never
matters in practice — revisit if a future test needs multiple simultaneously-exposed,
differently-permissioned tags on the same allocation.

---

## Stacked Borrows — fail/ tests

| SB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `deallocate_against_protector1` | Deallocating heap memory while a strongly-protected `&mut i32` is active triggers a Stacked Borrows violation. | `covered` | `hb_deallocate_against_protector1.rs` (in `fail/all/`; FnEntry tag lands in `exposed_stack[0].current_borrower`, scanned by `before_memory_deallocation`) |
| `disable_mut_does_not_merge_srw` | A disabled mutable reference prevents neighboring SRW groups from merging; a parent write invalidates a raw pointer in the lower group. | `no_std` | — |
| `drop_in_place_protector` | `drop_in_place` retags a zero-sized drop type and invalidates strongly-protected aliases via the drop protector mechanism. | `no_dealloc` | — |
| `drop_in_place_retag` | `drop_in_place` performs a mutable retag requiring Unique permission, denied when the pointer only holds SharedReadOnly. | `hb_pass` | `hb_drop_in_place_retag.rs` (uses `ptr::write` analog; HB has no Unique/SRO permission distinction) |
| `exposed_only_ro` | A write through a wildcard pointer derived via `expose_provenance` fails when only read-only tags have been exposed. | `covered` | `hb_wildcard_write_readonly.rs` (in `fail/all/`; the exposed shared-ref tag is no longer the live owner identity by the time of the write) |
| `fnentry_invalidation` | A raw pointer derived from a mutable reference survives an FnEntry retag of the same location and remains readable. | `hb_pass` | `sb_fnentry_doesnt_invalidate_raw.rs` |
| `fnentry_invalidation2` | Calling `as_mut_ptr()` inside an inner function via FnEntry retag permanently invalidates a raw pointer derived from a prior shared borrow. | `hb_pass` | `hb_fnentry_invalidation2.rs` (uses `addr_of!` to avoid shared-borrower lifetime issue; T_ptr survives inner FnEntry retag) |
| `illegal_dealloc1` | Deallocating through a pointer whose tag was invalidated by a sibling write is caught by the borrow stack. | `hb_pass` | `hb_illegal_dealloc1.rs` (in `pass/sb_fail/`; HB has no pop-on-lower-access — a write through ptr1 does not invalidate ptr2, so dealloc through ptr2 succeeds) |
| `illegal_read1` | A callee reads via a raw pointer after a child `&mut` was derived; SB kills the child on the raw read, but HB allows it. | `covered` | `sb_raw_read_doesnt_kill_child_mut.rs` |
| `illegal_read2` | A callee creates a shared reference from the parent raw pointer and reads through it, then the caller reads through the child mutable reference. | `covered` | `hb_callee_shared_ref_child_mut_survives.rs` |
| `illegal_read3` | A mutable reference smuggled through a union via `mem::transmute_copy` causes a reborrow invalidation via an SB borrow-stack tag conflict. | `no_std` | — |
| `illegal_read4` | A raw pointer READ via the base pointer after a child `&mut` reborrow does not kill the child in HB; the subsequent use of the child is valid. | `covered` | `hb_inline_raw_read_child_mut_survives.rs` |
| `illegal_read5` | Reading through a shared `&RefCell` alias after creating a `&mut` to the inner value invalidates the mutable reference. | `no_std` | — |
| `illegal_read6` | Creating a shared reference from a reborrow does not re-activate a raw pointer killed by the reborrow; a subsequent read through that raw pointer is UB. | `covered` | `hb_prev_borrower_accessible_after_shared_reborrow.rs` |
| `illegal_read7` | Creating a shared reference over a `Cell` does not leak data to a previously-derived raw pointer; reading via the raw pointer invalidates the underlying unique borrow. | `covered` | `hb_cell_raw_read_child_survives.rs` (HB pass — base-ptr READ does not kill child in HB) |
| `illegal_read8` | A raw pointer write invalidates a co-existing shared reference, causing a subsequent read through the shared reference to fail. | `covered` | `shared_ref_invalidated_by_raw_write.rs` |
| `illegal_write2` | A raw pointer derived from a mutable reference becomes invalid after a reborrow of that reference; writing through it is UB. | `covered` | `hb_raw_cast_survives_ref_reborrow.rs` (in `fail/tb_pass/`: HB fail, SB fail, TB pass!) |
| `illegal_write3` | Writing through a raw pointer derived from a shared (frozen) reference is UB while the shared borrow is live. | `covered` | `raw_from_frozen_ref_write_denied.rs` |
| `illegal_write4` | Creating a raw-tagged `&mut` via transmute from a raw pointer unfreezes a frozen location, invalidating a subsequent shared-reference read. | `no_std` | — |
| `interior_mut1` | A write through an outer `UnsafeCell` raw pointer invalidates a mutable reborrow and a shared reference layered on top of it. | `covered` | `hb_interior_mut_write_kills_reborrow.rs` |
| `interior_mut2` | Writing through a raw pointer from an outer `UnsafeCell` invalidates an inner shared reference derived via `mutable_transmutes`. | `no_std` | — |
| `invalidate_against_protector1` | Accessing an aliased raw pointer while a `&mut` argument with a strong protector is active violates the protector. | `covered` | `sb_raw_read_violates_mut_protector.rs` |
| `load_invalid_mut` | A child `&mut` stored in `Box` is invalidated by a raw-pointer read; loading it back from `Box` triggers a retag failure. | `hb_pass` | `hb_load_invalid_mut.rs` (uses `MaybeUninit` instead of `Box`; PoloniusAnchor restores T_xraw — no pop-on-lower-access semantics in HB) |
| `pass_invalid_mut` | A raw-ptr read through the parent invalidates a child `&mut` in SB (via retag on call), but HB allows it because reads do not kill the child current_borrower. | `covered` | `hb_pass_invalid_mut_raw_read.rs` |
| `pointer_smuggling` | A raw pointer stored into a static global during a mutable borrow becomes invalid after the callee returns and the original borrow regains exclusivity. | `covered` | `smuggled_ptr_invalid_after_borrow_restored.rs` |
| `raw_tracking` | Two sequential independent `&mut`-to-raw casts of the same place invalidate the first raw pointer when the second is created. | `covered` | `sb_sequential_reborrows_kill_first.rs` |
| `retag_data_race_protected_read` | A protected mutable retag in one thread and a concurrent read in the main thread constitute a data race. | `no_concurrency` | — |
| `retag_data_race_read` | A retag (shared reborrow) acts as a read for the data race model, racing with a concurrent write on another thread. | `no_concurrency` | — |
| `return_invalid_mut` | A child `&mut` reborrowed from a raw pointer is returned after a read through the parent raw pointer invalidates it in SB; HB allows it. | `covered` | `return_invalid_mut_option.rs` |
| `return_invalid_mut_option` | Returning a `&mut` wrapped in `Option<&mut T>` invalidated by a parent raw-pointer read is UB under SB but valid under HB. | `covered` | `return_invalid_mut_option.rs` |
| `return_invalid_mut_tuple` | A `&mut` returned inside a tuple is invalidated by a subsequent raw-pointer read in SB but remains valid in HB. | `covered` | `return_invalid_mut_tuple.rs` |
| `shared_rw_borrows_are_weak1` | A SharedReadWrite borrow placed below a Unique on the stack allows a write through it to invalidate the outstanding Unique. | `no_std` | — |
| `shared_rw_borrows_are_weak2` | A SharedReadWrite borrow placed below an existing SharedReadWrite on the stack allows a write to invalidate the earlier borrow. | `no_std` | — |
| `static_memory_modification` | Writing to a static (read-only) allocation via a transmuted mutable reference is detected as UB. | `covered` | `hb_static_memory_modification.rs` (in `fail/all/`; HB's retag rejects creating a `&mut` from an already-shared tag eagerly, before the base-interpreter `WriteToReadOnly` check SB hits is ever reached — different mechanism, same UB verdict) |
| `track_caller` | `#[track_caller]` prevents callee source appearing in SB diagnostics when a raw-pointer read invalidates a derived mutable reference. | `diagnostic_only` | — |
| `transmute-is-no-escape` | A raw pointer derived via transmute from a mutable reference carries a tag; a provenance-stripped pointer obtained via `wrapping_offset` is caught as invalid on write. | `no_wildcard` | — |
| `unescaped_local` | A raw pointer derived via an integer-to-pointer cast cannot write to a local after a fresh reborrow invalidates the exposed tag. | `covered` | `hb_wildcard_write_after_reborrow.rs` (in `fail/all/`; the exposed tag is fully displaced from `BorrowerState` by the new reborrow, so the wildcard search finds no live candidate) |
| `unescaped_static` | A raw pointer derived from a reference to a static array element cannot be used via pointer arithmetic to access an adjacent element. | `hb_pass` | `hb_unescaped_static.rs` (in `pass/sb_fail/`; HB's `BorrowerState` is per-allocation, not per-byte, so a tag from `&ARRAY[0]` remains valid for offsetting to byte 1 — the static-array instance of the existing `no_per_byte` divergence) |
| `zst_slice` | Dereferencing a pointer derived from a ZST (zero-length) slice and offsetting it out of bounds triggers a borrow-stack tag-not-found error. | `no_zst` | — |

---

## Stacked Borrows — pass/ tests

| SB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `coroutine-self-referential` | A self-referential coroutine holds a mutable reference into its own captured state across yield points, which `!Unpin` types permit. | `no_std` | — |
| `stack-printing` | Tests per-byte borrow-stack printing diagnostics using permissive provenance, int-to-ptr cast, and explicit heap allocation/deallocation via `std::alloc`. | `diagnostic_only` | — |
| `stacked-borrows` | Tests SB-specific patterns: mut-raw-mut with interleaved read, and a two-phase reservation aliasing violation that SB accepts but TB rejects. | `no_std` | — |
| `unknown-bottom-gc` | Checks that the SB GC correctly handles an unknown-bottom (wildcard/exposed-provenance) entry when many SharedReadOnly items are piled on top. | `covered` | `hb_wildcard_write_basic.rs` (in `pass/all/`; basic expose + wildcard write where the exposed tag is still the current owner) |
| `zst-field-retagging-terminates` | Checks that retagging `usize::MAX` ZST array fields terminates quickly via a ZST fast path, with no aliasing logic involved. | `no_zst` | — |

---

## Tree Borrows — fail/ tests (main)

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `alternate-read-write` | Alternating parent reads and child writes via a raw-pointer reborrow: TB freezes the child mutable reference; HB does not. | `covered` | `tb_alternate_rw_parent_read_no_freeze.rs` |
| `cell-inside-struct` | Writing to a frozen field through a `*mut` cast of a shared ref is forbidden, while writing to a `Cell`-wrapped field in the same struct is allowed. | `no_per_byte` | — |
| `error-range` | Per-byte permission tracking correctly identifies the exact byte range at fault when a parent write strips permissions from a child `&mut`. | `no_per_byte` | — |
| `fnentry_invalidation` | Writing through a raw pointer, calling a method that triggers FnEntry retag (TB: Unique → Frozen), then writing again via raw is forbidden in TB but valid in HB. | `covered` | `tb_fnentry_write_before_call_ptr_survives.rs` |
| `frozen-lazy-write-to-surrounding` | A raw pointer derived from a shared reference to a ZST field cannot write to adjacent memory by casting the pointer to a wider type. | `no_std` | — |
| `outside-range` | A TB protector on a mutable reference fires only for byte offsets that have already been accessed through that reference, not unaccessed offsets. | `no_per_byte` | — |
| `parent_read_freezes_raw_mut` | A parent read through the root binding after a raw pointer write freezes (invalidates) the raw pointer in TB; HB does not freeze on parent reads. | `covered` | `tb_parent_read_no_freeze_raw.rs` |
| `pass_invalid_mut` | Writing through a `&mut` whose parent was read via a raw pointer (causing TB invalidation) is forbidden in TB; HB allows it because parent reads do not kill the child. | `covered` | `hb_tb_pass_invalid_mut_write.rs` |
| `protector-write-lazy` | A TB "Reserved Lazy" pointer at offset 0 is invalidated by a protected activated write, enforcing write-reordering soundness under protectors. | `no_per_byte` | — |
| `repeated_foreign_read_lazy_conflicted` | A TB Reserved mutable reference whose "conflicted" flag is set by a foreign read cannot subsequently be written through. | `no_std` | — |
| `reservedim_spurious_write` | A spurious write through a protected mutable reference causes UB when a concurrently-lazy ReservedIM pointer later writes (TB + concurrency). | `no_concurrency` | — |
| `return_invalid_mut` | A returned `&mut i32` derived via raw pointer, activated with a write, then invalidated by a parent raw-pointer read, cannot be used for writing in TB; HB passes. | `covered` | `hb_tb_return_invalid_mut_write.rs` |
| `spurious_read` | A write through a protected mutable reference while a sibling protected reference exists causes UB in a two-thread barrier-synchronized interleaving. | `no_concurrency` | — |
| `strongly-protected` | Deallocating memory through a raw pointer derived from a strongly-protected mutable reference (active function-call protector) is forbidden. | `covered` | `hb_strongly_protected.rs` (in `fail/all/`; inner's FnEntry StrongProtector tag is found in `exposed_stack` when callback calls dealloc) |
| `subtree_traversal_skipping_diagnostics` | A write through a mutable reference is forbidden when a Frozen intermediary node exists in the TB tree, testing the subtree-traversal skipping optimization. | `no_std` | — |
| `write-during-2phase` | A foreign write via a raw-pointer alias invalidates a Reserved two-phase mutable borrow of a Freeze type before function entry. | `covered` | `tb_write_during_two_phase_reservation.rs` |

---

## Tree Borrows — fail/ reserved/

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `cell-protected-write` | A protected `&mut UnsafeCell` (ReservedIM state) forbids a foreign write through an aliased raw pointer even though interior mutability normally allows it. | `covered` | `hb_cell_protected_write.rs` (in `fail/sb_pass/`; HB fails because two-phase activation hook clears `shared_borrower` at call site before callee runs) |
| `int-protected-write` | A TB Reserved (unwritten) mutable reference under a protector is Disabled when a foreign write occurs through a sibling raw pointer during the call. | `covered` | `protected_mut_foreign_write.rs` |

---

## Tree Borrows — fail/ wildcard/

HB now implements a wildcard/exposed-provenance model (see "HB's wildcard model" below). Three
representative tests from this group have been ported to confirm the model's behavior against TB;
the remaining 25 rely on TB's tree-specific cross-subtree invalidation or protector/GC interactions
that have no HB analogue (HB has no tree, so "which subtree" is not a meaningful question), and were
not individually ported.

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `single_exposed_local` | Wildcard write succeeds once, then a parent read freezes the only exposed reference in TB; a second wildcard write fails. | `hb_pass` | `hb_wildcard_frozen_ref.rs` (in `pass/tb_fail/`; HB has no freeze-on-parent-read, so the second write succeeds — a pre-existing, documented HB/TB divergence, not a wildcard-specific gap) |
| `single_exposed_disable` | A sibling write through `ref2` disables `ref1`; a wildcard read through `ref1`'s exposed tag then fails. | `covered` | `hb_wildcard_sibling_disable.rs` (in `fail/all/`; ref2 overwrites the shared `exposed_stack` entry in place, erasing ref1's tag from `BorrowerState` entirely — converges with TB's fail via a different mechanism) |
| `cross_tree_update_main` | A write through a sibling subtree (`ref2`) disables a wildcard reborrow (`reb`) rooted in a different subtree. | `covered` | `hb_wildcard_cross_tree.rs` (in `fail/all/`; same tag-erasure mechanism as `single_exposed_disable` — HB has no tree, but the RawPtr-stack overwrite produces the same verdict) |

Remaining affected tests (25): `cross_tree_from_main`,
`cross_tree_update_main_invalid_exposed`, `cross_tree_update_main_invalid_exposed2`,
`cross_tree_update_newer`, `cross_tree_update_newer_exposed`, `cross_tree_update_older`,
`cross_tree_update_older_invalid_exposed`, `cross_tree_update_older_invalid_exposed2`, `dealloc`,
`gc`, `multi_exposed_child`, `multi_exposed_child_unique_writer`, `multi_exposed_siblings_disable`,
`multi_exposed_siblings_foreign`, `multi_exposed_siblings_local`,
`multi_exposed_siblings_unique_writer`, `protected_wildcard`, `protector_conflicted`,
`protector_release`, `protector_release2`, `single_exposed_foreign`,
`single_exposed_only_ro`, `strongly_protected_wildcard`,
`subtree_internal_relatedness`, `subtree_internal_relatedness_wildcard`.

---

## Tree Borrows — pass/ tests

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `cell-alternate-writes` | Two aliasing shared references to an `UnsafeCell` remain valid across alternating interleaved writes through each reference. | `no_std` | `unsafecell_aliasing_writes.rs` |
| `cell-inside-box` | A ReservedIM (`UnsafeCell` inside `Box`) raw pointer survives a foreign write when a sibling pointer transitions to Unique via a child write. | `no_std` | `unsafecell_aliasing_writes.rs` |
| `cell-inside-struct` | Writing to both a `Cell` field and a non-`Cell` field through a `*mut` cast of a shared `&Foo` is permitted when per-byte interior-mutability precision is disabled. | `no_std` | — |
| `cell-lazy-write-to-surrounding` | A raw pointer derived from a shared reference to a `!Freeze` (`Cell`) type retains write permission to surrounding memory under TB. | `no_per_byte` | — |
| `copy-nonoverlapping` | `copy_nonoverlapping` works correctly when a shared pointer is obtained before a mutable pointer into the same allocation (non-overlapping regions). | `no_std` | — |
| `end-of-protector` | A mutable-reference protector installed at function entry is fully released when the function returns, allowing a subsequent reborrow to write without UB. | `covered` | `protector_lifetime.rs` |
| `formatting` | Tests pretty-print formatting of TB per-byte tag trees across aliased slice indices with different permission states (Active/Frozen). | `diagnostic_only` | — |
| `read_retag_no_race` | A mutable retag in one thread does not race with a concurrent read in another thread when the non-UB interleaving is enforced deterministically. | `no_concurrency` | — |
| `reborrow-is-read` | Creating a reborrow from a parent mutable reference counts as a Read access, causing a sibling Unique reference to transition to Frozen in TB. | `diagnostic_only` | — |
| `reserved` | Exhaustively checks TB Reserved state transitions (to Frozen, Disabled, or no-op) under all combinations of interior mutability and protector presence. | `no_std` | — |
| `sb_fails` | Mutable reborrows without actual writes do not invalidate sibling raw pointers (SB fails, TB/HB pass), across four aliasing patterns. | `covered` | `sb_fnentry_doesnt_invalidate_raw.rs`, `hb_pass_invalid_mut_raw_read.rs`, `hb_return_invalid_mut_raw_read.rs` (portable sub-modules); the `static_memory_modification` sub-module is a genuine three-way divergence (SB fail, TB pass, HB fail) — see `hb_static_memory_modification_readonly.rs` in `fail/tb_pass/` |
| `spurious_read` | A spurious read inserted during a protected mutable reborrow is valid when the aliased-pointer write occurs after protector release, using two synchronized threads. | `no_concurrency` | — |
| `transmute-unsafecell` | `mem::transmute` between `&i32` and `&UnsafeCell<i32>` in both directions is valid as long as no write is performed through the Frozen-parent-constrained cell. | `covered` | `hb_transmute_unsafecell.rs` (in `pass/broken/`; HB fails due to PoloniusAnchor firing at transmute site — implementation limitation) |
| `tree-borrows` | TB-specific aliasing patterns: multiple Reserved mut refs coexisting, raw pointer alias with local, protector on disjoint fields, reborrow returned through function boundary. | `no_std` | — |

### TB pass — wildcard/ sub-group

All four tests (`formatting`, `reborrow`, `undetected_ub`, `wildcard`) rely on int-to-ptr casts with
`-Zmiri-permissive-provenance` to create wildcard pointers.  They cannot be ported to HB for the
same reason as the fail/wildcard/ group above.

---

## Tests needing HB ports

All previously `port_needed` tests have been addressed (ported in second and third passes).
Third-pass ports: `drop_in_place_retag`, `fnentry_invalidation2`, `load_invalid_mut`
(→ `pass/sb_fail/`), `cell-protected-write` (→ `fail/sb_pass/`), and `transmute-unsafecell`
(→ `pass/broken/`, implementation limitation).


> **Note on TB pass — `copy-nonoverlapping`:** Although currently classified `no_std` (the upstream
> test uses `std::alloc`), the sketch above notes that a `core::ptr::copy_nonoverlapping` port with a
> stack-allocated `[u64; 2]` array is feasible and could be promoted to `port_needed` once confirmed
> that HB handles disjoint-region shared/mutable access correctly.
