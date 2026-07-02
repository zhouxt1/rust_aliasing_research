# Hybrid Borrows Test Coverage Map

This document maps every test in Miri's Stacked Borrows (SB) and Tree Borrows (TB) test suites to its
coverage status in the Hybrid Borrows (HB) test suite.  For each upstream test the table records
whether the aliasing scenario is already exercised by an HB test, whether it cannot be ported due to
a feature HB does not implement, or whether a new HB test still needs to be written.

---

## Summary

| Status | Meaning | Count |
|---|---|---|
| `covered` | An existing HB test exercises the same aliasing scenario | 62 |
| `hb_pass` | HB semantics differ from SB/TB (HB allows the program); pattern documented in pass/ | 11 |
| `no_dealloc` | Requires deallocation-tracking semantics not implemented in HB | 1 |
| `no_wildcard` | Requires wildcard / exposed-provenance tag support not implemented in HB | 19 |
| `no_per_byte` | Requires per-byte permission granularity not implemented in HB | 5 |
| `no_concurrency` | Requires multi-thread concurrency not implemented in HB | 5 |
| `diagnostic_only` | Only validates error-message formatting; no aliasing logic to port | 4 |

`port_needed`, `no_zst`, `no_static`, and `no_std` are no longer listed — all four categories have
been fully resolved (0 remaining) and every row that used to carry those statuses now shows
`covered` or `hb_pass` with its HB counterpart.

**`RawPointerStack.dead` fix (2026-07-02):** `load_invalid_mut` and `repeated_foreign_read_lazy_conflicted`
moved from `hb_pass` to `covered` (HB's verdict now matches SB/TB, at least for the former; the
latter's convergence is likely coincidental). Net effect on this table: `covered` 60 → 62, `hb_pass`
13 → 11. This is a small fraction of the fix's actual footprint — it also fixed 3 more tests whose
rows were already (mis-)labeled `covered` before and after, and caused 10 regressions across rows
that remain `covered`/`hb_pass` in label but whose HB counterpart file moved and whose behavior
changed. See the "Case study" section below for the full picture; counts alone understate what
changed here.

**`no_std` audit (2026-06-30 to 2026-07-01):** none of the 17 tests then labeled `no_std` actually
needed `std` — the categorization heuristic apparently just checked for a `use std::...` import,
but `Cell`, `RefCell`, `UnsafeCell`, `mem::transmute`/`transmute_copy`/`forget`, `ptr::read`/
`addr_of_mut`, and even `core::ops::{Coroutine, CoroutineState}` are all available from `core`.
12 tests ported cleanly with the expected verdict on the first try. 2 more (`cell-alternate-writes`,
`cell-inside-box`) already had HB counterparts but were left mislabeled — corrected to `covered`.
The remaining 3 (`coroutine-self-referential`, `reserved`, `tree-borrows`) surfaced **new, genuine
HB findings** while porting, unrelated to the `no_std` question itself:

- **Sibling-reborrow displacement** (`reserved`'s 2 "protected read" sub-scenarios,
  `tree-borrows`'s `aliasing_read_only_mutable_refs`): two sequential `&mut *base` reborrows of
  the same raw pointer cause the second to displace the first's tag entirely (the rule added to
  fix `hb_wildcard_sibling_disable.rs`). TB tolerates sibling Reserved nodes coexisting read-only;
  HB, having no tree, cannot — a real, reproducible HB/TB divergence, not a porting artifact.
- **Coroutine resume state**: `coroutine-self-referential` compiles and runs quickly (no hang),
  but `reborrow_chain` accumulates an entry on every `resume()` without ever being cleared between
  yields, eventually causing a stale-tag access failure. A real gap in HB's reborrow-chain
  bookkeeping for resumable state machines, a MIR shape unlike anything else in this corpus.
- **`copy-nonoverlapping`**: `test_from_to` fails because `data.as_ptr()` leaves a lingering
  `shared_borrower` via HB's Polonius tracking, conflicting with the later `data.as_mut_ptr()` —
  the same `as_ptr()`-vs-Polonius-lifetime issue noted elsewhere in this corpus.

`tree-borrows`'s `string_as_mut_ptr` sub-function remains unported: it needs
`alloc::string::String`, which shares `Box`'s `RawVec`/allocator-lang-item machinery, already
known to hit the Polonius-MIR-for-stdlib `Layout` validation bug from an earlier session — the one
genuinely std-blocked case in the whole audit.

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
grants.

**Multiple-simultaneously-exposed-tags batch (2026-07-01):** the `multi_exposed_*` and
`protector_*` wildcard tests exercise exactly this simplification's edge — multiple tags exposed
on the same allocation at once. In practice, though, almost every one of these tests is dominated
by a *different*, pre-existing mechanism before the multi-tag search logic ever gets meaningfully
exercised: **sibling-reborrow displacement** (the rule added to fix `hb_wildcard_sibling_disable.rs`,
where a second `&mut *base` reborrow of the same raw pointer overwrites the first's `exposed_stack`
entry). Since most of these tests derive their multiple exposed references sequentially from one
`ptr_base`, the earlier ones are already gone from `BorrowerState` by the time the wildcard access
happens — so the search finds zero or one live candidate, not a real multi-candidate choice. The
two tests using a *Ref-source chain* instead of raw-pointer siblings (`multi_exposed_child`,
`multi_exposed_child_unique_writer`) hit a different pre-existing rule: `resolve_wildcard_tag` can
find an ancestor's tag surviving in `reborrow_chain`, but chain-only tags only ever grant reads,
not writes — so the wildcard write fails even though resolution technically succeeded. Net result:
this batch didn't find a case where the "take the first candidate, don't backtrack" simplification
itself produces a wrong answer — every divergence traces to one of these two already-known rules.
That question remains open for a future test that keeps multiple *actually-independent* (not
sibling-displaced) tags alive and exposed on one allocation at once.

**Protector+wildcard finding:** `strongly_protected_wildcard` confirms `before_memory_deallocation`
needs no wildcard-specific handling — it already scans every tag in `BorrowerState` regardless of
how the deallocating address was derived. `protector_release`/`protector_release2` both produce UB
matching TB's verdict, via plain tag mismatches rather than tree-based reasoning.

**Genuine bug found and fixed (2026-07-01):** `protected_wildcard.rs` initially triggered a compiler
crash (ICE) — `compute_retags` (`miri.rs:283`) called `place.is_indirect_first_projection()` on the
*raw* borrowck body for user code, before `run_analysis_to_runtime_passes` runs. That method's own
doc comment says its invariant (`Deref` only ever first in the projection list) only holds from
`AnalysisPhase::PostCleanup` onward; on the raw body it can legitimately be violated (this test's
`FnMut` closure captures a reference *and* receives a dereferenced parameter, producing a `Deref`
later in the projection chain via closure-capture desugaring). Fixed by swapping in the
general-purpose `is_indirect()`, which the same doc comment confirms is equivalent post-cleanup and
also correct pre-cleanup. Verified against a 10-test regression sample spanning every major
mechanism in this corpus (dealloc protectors, ZST fast path, sibling-reborrow displacement,
two-phase borrows, protector conflicts) with zero behavior changes elsewhere. With the crash gone,
`protected_wildcard` resolves to a normal `hb_pass` result — see the table below.

---

## Case study: `pass/sb_tb_fail` — parent reads, the `RawPointerStack.dead` fix, and its regressions

**Status as of 2026-07-02: the fix below is implemented and built. The `activated`/Reserved-vs-Active
refinement described at the end is NOT implemented — deliberately deferred pending a decision on
whether to extend the model further.**

### The original problem

The `pass/sb_tb_fail/` directory held 7 tests where HB *incorrectly passed* a program that **both**
SB and TB independently reject as UB — the strongest possible signal of a genuine HB soundness gap,
since it can't be dismissed as "just a documented HB/TB divergence" (those get `hb_pass`/`covered`
elsewhere in this document; two different aliasing models agreeing means the pattern really is
unsound). All 7 shared one root mechanism: **a parent read through a raw pointer did not
freeze/kill a child `&mut` borrow tracked in HB's `RawPointerStack`.** SB pops the child's stack
item on any foreign access, read or write, with no exceptions; TB freezes an *Active* (already
self-written) child on a foreign read. HB's `check_raw_pointer_stack` had no equivalent: a read
through an entry's `base_pointer` was a pure no-op.

### The fix: binary `dead` state

`RawPointerStack` gained a `dead: bool` field (see `borrower.rs`). A **read** through an entry's
`base_pointer` now marks that entry, and everything above it in the stack, `dead`. This is
deliberately a **binary** alive/dead state, not TB's softer Frozen (which still permits reads) —
once dead, *any* further access through the entry, read or write, is denied. A fresh reborrow from
the same `base_pointer` (the "second reborrow from the same base" branch in
`apply_reborrow_to_stack`) revives the slot (`dead = false`); a plain `Ref`-source retag that only
renames the tag in place does *not* revive it, since that's still logically the same dead borrow
wearing a new name.

### Results: 5 fixed, 2 still unfixed (different mechanism)

Of the 7 original tests, **5 are now fixed** — moved to `fail/all/` with updated doc comments:
`hb_tb_pass_invalid_mut_write.rs`, `hb_tb_return_invalid_mut_write.rs`,
`return_invalid_mut_option.rs`, `return_invalid_mut_tuple.rs`,
`tb_alternate_rw_parent_read_no_freeze.rs`. Two remain unfixed, still in `pass/sb_tb_fail/`:
`tb_fnentry_write_before_call_ptr_survives.rs` and `tb_parent_read_no_freeze_raw.rs`. Both use a
**bare `existing_mut_ref as *mut T` cast** rather than a `&mut *raw_ptr` reborrow. Per the existing
code comment in `mod.rs` ("Like SB, raw pointers are only retagged for `RetagKind::Raw`"), casting
FROM an existing reference creates no new tracked identity at all — the raw pointer just carries
the same tag as `current_borrower` directly, so there is no separate `RawPointerStack` entry for
the `dead` fix to act on. Fixing these needs a deeper structural change (making bare-cast raw
pointers trackable in their own right), not just the `RawPointerStack`-local fix implemented here.

One fixed test has a **timing mismatch**: `tb_alternate_rw_parent_read_no_freeze.rs` now fails at
the *first* `*y += 1` (after only one prior read), whereas TB itself only fails at the *second*
write (after two reads) — because TB's Reserved state tolerates the first foreign read, and only
an Active (already-written) node gets frozen on a foreign read. Both models reject the program, but
HB is more conservative than necessary. This timing gap is a direct symptom of the same missing
distinction described next.

### The regression: 10 tests, missing Reserved-vs-Active distinction

A full 97-test regression sweep (`run_tests.sh` over the whole corpus) surfaced **10 previously
correctly-passing tests that now incorrectly fail.** All 10 share one root cause: TB's Reserved
(never self-written) state tolerates foreign reads/reborrows freely — only an Active
(already self-written) node gets frozen by a foreign read. HB's `dead` flag makes no such
distinction: it kills a `RawPointerStack` entry on ANY read through its base pointer, whether or
not the entry was ever activated. Confirmed by hand-tracing `hb_pass_invalid_mut_raw_read.rs`
(never writes `xref` before the parent read) against a debug log showing the entry killed
immediately on the read, then the FnEntry retag denied — even though nothing was ever written
through `xref`.

The 10 regressed tests (moved to `fail/tb_pass/`, or `fail/all/` for the one uncertain case, each
with a doc-comment note explaining the regression):

- `hb_reserved_unprotected.rs` (4 sub-scenarios) — pure TB-sourced (Reserved-tolerates-foreign-access
  battery), no SB counterpart.
- `hb_callee_shared_ref_child_mut_survives.rs`, `hb_inline_raw_read_child_mut_survives.rs`,
  `hb_load_invalid_mut.rs`, `hb_pass_invalid_mut_raw_read.rs`, `hb_return_invalid_mut_raw_read.rs`,
  `sb_pass_reserved_ref_after_parent_read.rs`, `sb_raw_read_doesnt_kill_child_mut.rs`,
  `sb_return_reserved_ref_after_parent_read.rs` — all SB-fail/HB-used-to-pass tests where the child
  reference was never activated before the parent read.
- `hb_repeated_foreign_read_lazy_conflicted.rs` — a **different, uncertain case**: this was a
  *documented, deliberate* HB/TB divergence (HB has no "conflicted" flag concept), not a target of
  the original fix. It now fails too, converging with TB's verdict, but very likely by accident —
  via the same blunt kill-on-any-read rule, not a real implementation of TB's conflicted-flag
  semantics. Its neighboring reference is also never activated, so it may well flip back to passing
  if the `activated` refinement below is implemented. Kept separate from the other 9 clear
  regressions; do not read its current "fail" as validated the way the other fixes are.

### Proposed refinement (NOT implemented — pending decision)

The fix verified by hand-trace, but deliberately not yet applied: add an `activated: bool` field to
`RawPointerStack`, set `true` the first time a **write** occurs through `raw_pointer_borrower`
(mirroring TB's Reserved→Active transition). The `dead`-on-parent-read rule would only fire when
`activated` is already `true` — an un-activated (Reserved) entry tolerates parent reads exactly as
TB tolerates them on Reserved nodes.

This was verified by hand-trace to:
1. Still catch all 5 originally-fixed target tests (each writes through the child before the parent
   read that's supposed to invalidate it).
2. Avoid all 10 regressions (each depends on the child never being activated before the parent
   read).
3. Make `tb_alternate_rw_parent_read_no_freeze.rs` match TB's *exact* staggered timing (fail at the
   second write, not the first), since `*y += 1` on the first write activates `y` for the first
   time, and only the *second* subsequent read would then kill it.

This directly parallels HB's existing `NewPermission::TwoPhase` mechanism (Reserved/ReservedIM
permission states for compiler-inserted two-phase borrows) — the same Reserved-vs-Active idea,
just applied to `RawPointerStack` entries generally rather than only at two-phase-borrow call
sites. Whether to extend HB's model this far — adding a second bit of state to every
`RawPointerStack` entry, purely to more closely track TB's nuance — is an open design question, not
yet decided.

---

## Stacked Borrows — fail/ tests

| SB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `deallocate_against_protector1` | Deallocating heap memory while a strongly-protected `&mut i32` is active triggers a Stacked Borrows violation. | `covered` | `hb_deallocate_against_protector1.rs` (in `fail/all/`; FnEntry tag lands in `exposed_stack[0].current_borrower`, scanned by `before_memory_deallocation`) |
| `disable_mut_does_not_merge_srw` | A disabled mutable reference prevents neighboring SRW groups from merging; a parent write invalidates a raw pointer in the lower group. | `covered` | `hb_disable_mut_does_not_merge_srw.rs` (in `fail/all/`; was miscategorized `no_std` — the test is pure `core`, uses only raw pointers) |
| `drop_in_place_protector` | `drop_in_place` retags a zero-sized drop type and invalidates strongly-protected aliases via the drop protector mechanism. | `no_dealloc` | — |
| `drop_in_place_retag` | `drop_in_place` performs a mutable retag requiring Unique permission, denied when the pointer only holds SharedReadOnly. | `hb_pass` | `hb_drop_in_place_retag.rs` (uses `ptr::write` analog; HB has no Unique/SRO permission distinction) |
| `exposed_only_ro` | A write through a wildcard pointer derived via `expose_provenance` fails when only read-only tags have been exposed. | `covered` | `hb_wildcard_write_readonly.rs` (in `fail/all/`; the exposed shared-ref tag is no longer the live owner identity by the time of the write) |
| `fnentry_invalidation` | A raw pointer derived from a mutable reference survives an FnEntry retag of the same location and remains readable. | `hb_pass` | `sb_fnentry_doesnt_invalidate_raw.rs` |
| `fnentry_invalidation2` | Calling `as_mut_ptr()` inside an inner function via FnEntry retag permanently invalidates a raw pointer derived from a prior shared borrow. | `hb_pass` | `hb_fnentry_invalidation2.rs` (uses `addr_of!` to avoid shared-borrower lifetime issue; T_ptr survives inner FnEntry retag) |
| `illegal_dealloc1` | Deallocating through a pointer whose tag was invalidated by a sibling write is caught by the borrow stack. | `hb_pass` | `hb_illegal_dealloc1.rs` (in `pass/sb_fail/`; HB has no pop-on-lower-access — a write through ptr1 does not invalidate ptr2, so dealloc through ptr2 succeeds) |
| `illegal_read1` | A callee reads via a raw pointer after a child `&mut` was derived; SB kills the child on the raw read. | `covered` | `sb_raw_read_doesnt_kill_child_mut.rs` (in `fail/tb_pass/`; **regressed 2026-07-02** by the `RawPointerStack.dead` fix — now fails too, matching SB/TB, but only because the child was never activated; see "pass/sb_tb_fail case study" below) |
| `illegal_read2` | A callee creates a shared reference from the parent raw pointer and reads through it, then the caller reads through the child mutable reference. | `covered` | `hb_callee_shared_ref_child_mut_survives.rs` (in `fail/tb_pass/`; **regressed 2026-07-02**, same mechanism — see case study) |
| `illegal_read3` | A mutable reference smuggled through a union via `mem::transmute_copy` causes a reborrow invalidation via an SB borrow-stack tag conflict. | `hb_pass` | `hb_illegal_read3.rs` (in `pass/sb_fail/`; was miscategorized `no_std` — `mem::transmute_copy` is `core`. HB does not track union-avoiding-retag invalidation the way SB does) |
| `illegal_read4` | A raw pointer READ via the base pointer after a child `&mut` reborrow; the subsequent use of the child is then tested. | `covered` | `hb_inline_raw_read_child_mut_survives.rs` (in `fail/tb_pass/`; **regressed 2026-07-02** — see case study) |
| `illegal_read5` | Reading through a shared `&RefCell` alias after creating a `&mut` to the inner value invalidates the mutable reference. | `covered` | `hb_illegal_read5.rs` (in `fail/all/`; was miscategorized `no_std` — `RefCell` is `core::cell::RefCell`) |
| `illegal_read6` | Creating a shared reference from a reborrow does not re-activate a raw pointer killed by the reborrow; a subsequent read through that raw pointer is UB. | `covered` | `hb_prev_borrower_accessible_after_shared_reborrow.rs` |
| `illegal_read7` | Creating a shared reference over a `Cell` does not leak data to a previously-derived raw pointer; reading via the raw pointer invalidates the underlying unique borrow. | `covered` | `hb_cell_raw_read_child_survives.rs` (HB pass — base-ptr READ does not kill child in HB) |
| `illegal_read8` | A raw pointer write invalidates a co-existing shared reference, causing a subsequent read through the shared reference to fail. | `covered` | `shared_ref_invalidated_by_raw_write.rs` |
| `illegal_write2` | A raw pointer derived from a mutable reference becomes invalid after a reborrow of that reference; writing through it is UB. | `covered` | `hb_raw_cast_survives_ref_reborrow.rs` (in `fail/tb_pass/`: HB fail, SB fail, TB pass!) |
| `illegal_write3` | Writing through a raw pointer derived from a shared (frozen) reference is UB while the shared borrow is live. | `covered` | `raw_from_frozen_ref_write_denied.rs` |
| `illegal_write4` | Creating a raw-tagged `&mut` via transmute from a raw pointer unfreezes a frozen location, invalidating a subsequent shared-reference read. | `covered` | `hb_illegal_write4.rs` (in `fail/all/`; was miscategorized `no_std` — `mem::transmute` is `core`. HB's shared→mutable retag rejection fires eagerly, same mechanism as `hb_static_memory_modification.rs`) |
| `interior_mut1` | A write through an outer `UnsafeCell` raw pointer invalidates a mutable reborrow and a shared reference layered on top of it. | `covered` | `hb_interior_mut_write_kills_reborrow.rs` |
| `interior_mut2` | Writing through a raw pointer from an outer `UnsafeCell` invalidates an inner shared reference derived via `mutable_transmutes`. | `covered` | `hb_interior_mut2.rs` (in `fail/all/`; was miscategorized `no_std` — `UnsafeCell`/`mem::transmute` are `core`) |
| `invalidate_against_protector1` | Accessing an aliased raw pointer while a `&mut` argument with a strong protector is active violates the protector. | `covered` | `sb_raw_read_violates_mut_protector.rs` |
| `load_invalid_mut` | A child `&mut` stored in `Box` is invalidated by a raw-pointer read; loading it back from `Box` triggers a retag failure. | `covered` | `hb_load_invalid_mut.rs` (uses `MaybeUninit` instead of `Box`; in `fail/tb_pass/`; **regressed 2026-07-02** — used to pass via PoloniusAnchor restoring T_xraw, now fails since the fix kills xref's entry unconditionally — see case study) |
| `pass_invalid_mut` | A raw-ptr read through the parent invalidates a child `&mut` in SB (via retag on call). | `covered` | `hb_pass_invalid_mut_raw_read.rs` (in `fail/tb_pass/`; **regressed 2026-07-02** — used to pass because reads did not kill the child current_borrower; now fails — see case study, this is the file used to diagnose the regression) |
| `pointer_smuggling` | A raw pointer stored into a static global during a mutable borrow becomes invalid after the callee returns and the original borrow regains exclusivity. | `covered` | `smuggled_ptr_invalid_after_borrow_restored.rs` |
| `raw_tracking` | Two sequential independent `&mut`-to-raw casts of the same place invalidate the first raw pointer when the second is created. | `covered` | `sb_sequential_reborrows_kill_first.rs` |
| `retag_data_race_protected_read` | A protected mutable retag in one thread and a concurrent read in the main thread constitute a data race. | `no_concurrency` | — |
| `retag_data_race_read` | A retag (shared reborrow) acts as a read for the data race model, racing with a concurrent write on another thread. | `no_concurrency` | — |
| `return_invalid_mut` | A child `&mut` reborrowed from a raw pointer is returned after a read through the parent raw pointer invalidates it in SB. | `covered` | `return_invalid_mut_option.rs` (in `fail/all/`; **FIXED 2026-07-02** — used to pass in HB, now fails at the return-site retag, matching SB — see case study) |
| `return_invalid_mut_option` | Returning a `&mut` wrapped in `Option<&mut T>` invalidated by a parent raw-pointer read is UB under SB. | `covered` | `return_invalid_mut_option.rs` (in `fail/all/`; **FIXED 2026-07-02** — see case study) |
| `return_invalid_mut_tuple` | A `&mut` returned inside a tuple is invalidated by a subsequent raw-pointer read in SB. | `covered` | `return_invalid_mut_tuple.rs` (in `fail/all/`; **FIXED 2026-07-02** — used to pass in HB, now fails at the return-site retag, matching SB — see case study) |
| `shared_rw_borrows_are_weak1` | A SharedReadWrite borrow placed below a Unique on the stack allows a write through it to invalidate the outstanding Unique. | `covered` | `hb_shared_rw_borrows_are_weak1.rs` (in `fail/all/`; was miscategorized `no_std` — `Cell`/`mem::transmute` are `core`) |
| `shared_rw_borrows_are_weak2` | A SharedReadWrite borrow placed below an existing SharedReadWrite on the stack allows a write to invalidate the earlier borrow. | `covered` | `hb_shared_rw_borrows_are_weak2.rs` (in `fail/all/`; was miscategorized `no_std` — `RefCell`/`mem::transmute` are `core`) |
| `static_memory_modification` | Writing to a static (read-only) allocation via a transmuted mutable reference is detected as UB. | `covered` | `hb_static_memory_modification.rs` (in `fail/all/`; HB's retag rejects creating a `&mut` from an already-shared tag eagerly, before the base-interpreter `WriteToReadOnly` check SB hits is ever reached — different mechanism, same UB verdict) |
| `track_caller` | `#[track_caller]` prevents callee source appearing in SB diagnostics when a raw-pointer read invalidates a derived mutable reference. | `diagnostic_only` | — |
| `transmute-is-no-escape` | A raw pointer derived via transmute from a mutable reference carries a tag; a provenance-stripped pointer obtained via `wrapping_offset` is caught as invalid on write. | `no_wildcard` | — |
| `unescaped_local` | A raw pointer derived via an integer-to-pointer cast cannot write to a local after a fresh reborrow invalidates the exposed tag. | `covered` | `hb_wildcard_write_after_reborrow.rs` (in `fail/all/`; the exposed tag is fully displaced from `BorrowerState` by the new reborrow, so the wildcard search finds no live candidate) |
| `unescaped_static` | A raw pointer derived from a reference to a static array element cannot be used via pointer arithmetic to access an adjacent element. | `hb_pass` | `hb_unescaped_static.rs` (in `pass/sb_fail/`; HB's `BorrowerState` is per-allocation, not per-byte, so a tag from `&ARRAY[0]` remains valid for offsetting to byte 1 — the static-array instance of the existing `no_per_byte` divergence) |
| `zst_slice` | Dereferencing a pointer derived from a ZST (zero-length) slice and offsetting it out of bounds triggers a borrow-stack tag-not-found error. | `hb_pass` | `hb_zst_slice.rs` (in `pass/sb_fail/`; SB's ZST retag never registers a real stack entry, so offsetting past it finds no tag; HB's per-allocation tracking still covers the offset via the original array's tag) |

---

## Stacked Borrows — pass/ tests

| SB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `coroutine-self-referential` | A self-referential coroutine holds a mutable reference into its own captured state across yield points, which `!Unpin` types permit. | `covered` | `hb_coroutine_self_referential.rs` (in `fail/sb_pass/`; compiles and runs quickly under `#![no_std]` — no hang. `reborrow_chain` accumulates a new entry on every `resume()` without being cleared between yields, and eventually a stale tag is neither `current_borrower` nor `prev_borrower`. A real gap in how HB's reborrow-chain bookkeeping interacts with resumable coroutine state machines) |
| `stack-printing` | Tests per-byte borrow-stack printing diagnostics using permissive provenance, int-to-ptr cast, and explicit heap allocation/deallocation via `std::alloc`. | `diagnostic_only` | — |
| `stacked-borrows` | Tests SB-specific patterns: mut-raw-mut with interleaved read, and a two-phase reservation aliasing violation that SB accepts but TB rejects. | `covered` | `hb_stacked_borrows.rs` (in `fail/sb_pass/`; was miscategorized `no_std` — pure `core`. HB fails the two-phase sub-pattern, siding with TB in a genuine 3-way divergence — same pattern as `hb_cell_protected_write.rs`) |
| `unknown-bottom-gc` | Checks that the SB GC correctly handles an unknown-bottom (wildcard/exposed-provenance) entry when many SharedReadOnly items are piled on top. | `covered` | `hb_wildcard_write_basic.rs` (in `pass/all/`; basic expose + wildcard write where the exposed tag is still the current owner) |
| `zst-field-retagging-terminates` | Checks that retagging `usize::MAX` ZST array fields terminates quickly via a ZST fast path, with no aliasing logic involved. | `covered` | `hb_zst_field_retagging_terminates.rs` (in `pass/all/`; confirmed HB's retag has a working ZST fast path — terminates in ~5s, not a hang) |

---

## Tree Borrows — fail/ tests (main)

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `alternate-read-write` | Alternating parent reads and child writes via a raw-pointer reborrow: TB freezes the child mutable reference. | `covered` | `tb_alternate_rw_parent_read_no_freeze.rs` (in `fail/all/`; **FIXED 2026-07-02**, but with a timing mismatch — HB now fails one round earlier than TB, since HB's binary `dead` flag has no Reserved-vs-Active distinction; see case study) |
| `cell-inside-struct` | Writing to a frozen field through a `*mut` cast of a shared ref is forbidden, while writing to a `Cell`-wrapped field in the same struct is allowed. | `no_per_byte` | — |
| `error-range` | Per-byte permission tracking correctly identifies the exact byte range at fault when a parent write strips permissions from a child `&mut`. | `no_per_byte` | — |
| `fnentry_invalidation` | Writing through a raw pointer, calling a method that triggers FnEntry retag (TB: Unique → Frozen), then writing again via raw is forbidden in TB but valid in HB. | `covered` | `tb_fnentry_write_before_call_ptr_survives.rs` (in `pass/sb_tb_fail/`; **still unfixed 2026-07-02** — the `RawPointerStack.dead` fix does not apply here, since `z = &mut x as *mut i32` is a bare cast with no `RawPointerStack` entry at all; a separate, deeper gap — see case study) |
| `frozen-lazy-write-to-surrounding` | A raw pointer derived from a shared reference to a ZST field cannot write to adjacent memory by casting the pointer to a wider type. | `covered` | `hb_frozen_lazy_write_to_surrounding.rs` (in `fail/all/`; the upstream test is fully `#![no_std]`-compatible — was miscategorized as `no_std`, actually belongs with the ZST tests. HB's Frozen permission is per-allocation, so `&pair.0` freezes the whole tuple, correctly denying the write regardless of per-byte granularity) |
| `outside-range` | A TB protector on a mutable reference fires only for byte offsets that have already been accessed through that reference, not unaccessed offsets. | `no_per_byte` | — |
| `parent_read_freezes_raw_mut` | A parent read through the root binding after a raw pointer write freezes (invalidates) the raw pointer in TB; HB does not freeze on parent reads. | `covered` | `tb_parent_read_no_freeze_raw.rs` (in `pass/sb_tb_fail/`; **still unfixed 2026-07-02** — `ptr = mref as *mut u8` is a bare cast, no `RawPointerStack` entry exists for the fix to act on; same bare-cast gap as `fnentry_invalidation` above — see case study) |
| `pass_invalid_mut` | Writing through a `&mut` whose parent was read via a raw pointer (causing TB invalidation) is forbidden in TB. | `covered` | `hb_tb_pass_invalid_mut_write.rs` (in `fail/all/`; **FIXED 2026-07-02** — used to pass in HB, now fails via the `RawPointerStack.dead` check, matching TB — see case study) |
| `protector-write-lazy` | A TB "Reserved Lazy" pointer at offset 0 is invalidated by a protected activated write, enforcing write-reordering soundness under protectors. | `no_per_byte` | — |
| `repeated_foreign_read_lazy_conflicted` | A TB Reserved mutable reference whose "conflicted" flag is set by a foreign read cannot subsequently be written through. | `covered` | `hb_repeated_foreign_read_lazy_conflicted.rs` (in `fail/all/`; **status uncertain since 2026-07-02** — HB used to pass here (documented divergence: no "conflicted" flag concept); after the `RawPointerStack.dead` fix HB now fails too, but very likely by accident, via the same blunt rule rather than any real conflicted-flag logic — see case study) |
| `reservedim_spurious_write` | A spurious write through a protected mutable reference causes UB when a concurrently-lazy ReservedIM pointer later writes (TB + concurrency). | `no_concurrency` | — |
| `return_invalid_mut` | A returned `&mut i32` derived via raw pointer, activated with a write, then invalidated by a parent raw-pointer read, cannot be used for writing in TB. | `covered` | `hb_tb_return_invalid_mut_write.rs` (in `fail/all/`; **FIXED 2026-07-02** — used to pass in HB, now fails at the return-site retag, one statement earlier than SB/TB's `*ret = 3` — see case study) |
| `spurious_read` | A write through a protected mutable reference while a sibling protected reference exists causes UB in a two-thread barrier-synchronized interleaving. | `no_concurrency` | — |
| `strongly-protected` | Deallocating memory through a raw pointer derived from a strongly-protected mutable reference (active function-call protector) is forbidden. | `covered` | `hb_strongly_protected.rs` (in `fail/all/`; inner's FnEntry StrongProtector tag is found in `exposed_stack` when callback calls dealloc) |
| `subtree_traversal_skipping_diagnostics` | A write through a mutable reference is forbidden when a Frozen intermediary node exists in the TB tree, testing the subtree-traversal skipping optimization. | `hb_pass` | `hb_subtree_traversal_skipping_diagnostics.rs` (in `pass/tb_fail/`; was miscategorized `no_std` — already core-only as written. HB has no tree/subtree structure, so there is nothing analogous to misbehave) |
| `write-during-2phase` | A foreign write via a raw-pointer alias invalidates a Reserved two-phase mutable borrow of a Freeze type before function entry. | `covered` | `tb_write_during_two_phase_reservation.rs` |

---

## Tree Borrows — fail/ reserved/

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `cell-protected-write` | A protected `&mut UnsafeCell` (ReservedIM state) forbids a foreign write through an aliased raw pointer even though interior mutability normally allows it. | `covered` | `hb_cell_protected_write.rs` (in `fail/sb_pass/`; HB fails because two-phase activation hook clears `shared_borrower` at call site before callee runs) |
| `int-protected-write` | A TB Reserved (unwritten) mutable reference under a protector is Disabled when a foreign write occurs through a sibling raw pointer during the call. | `covered` | `protected_mut_foreign_write.rs` |

---

## Tree Borrows — fail/ wildcard/

HB now implements a wildcard/exposed-provenance model (see "HB's wildcard model" below). 14
representative tests from this group have been ported to confirm the model's behavior against TB;
the remaining 14 rely on TB's tree-specific cross-subtree invalidation or protector/GC interactions
that have no HB analogue (HB has no tree, so "which subtree" is not a meaningful question), and were
not individually ported.

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `single_exposed_local` | Wildcard write succeeds once, then a parent read freezes the only exposed reference in TB; a second wildcard write fails. | `hb_pass` | `hb_wildcard_frozen_ref.rs` (in `pass/tb_fail/`; HB has no freeze-on-parent-read, so the second write succeeds — a pre-existing, documented HB/TB divergence, not a wildcard-specific gap) |
| `single_exposed_disable` | A sibling write through `ref2` disables `ref1`; a wildcard read through `ref1`'s exposed tag then fails. | `covered` | `hb_wildcard_sibling_disable.rs` (in `fail/all/`; ref2 overwrites the shared `exposed_stack` entry in place, erasing ref1's tag from `BorrowerState` entirely — converges with TB's fail via a different mechanism) |
| `cross_tree_update_main` | A write through a sibling subtree (`ref2`) disables a wildcard reborrow (`reb`) rooted in a different subtree. | `covered` | `hb_wildcard_cross_tree.rs` (in `fail/all/`; same tag-erasure mechanism as `single_exposed_disable` — HB has no tree, but the RawPtr-stack overwrite produces the same verdict) |
| `multi_exposed_child` | A wildcard write could be through an ancestor or a descendant of an exposed middle node; TB applies no transition since it can't tell which, so no error is expected. | `covered` | `hb_multi_exposed_child.rs` (in `fail/tb_pass/`; TB implicitly passes, HB fails at the first wildcard write — `resolve_wildcard_tag` finds the ancestor's tag in `reborrow_chain`, but chain-only tags only grant reads, not writes, an existing unrelated rule. Genuine, clean HB/TB divergence: HB is stricter here) |
| `multi_exposed_child_unique_writer` | A wildcard write can only be through the mutable ancestor (the descendant is frozen/shared); the write disables the descendant. | `covered` | `hb_multi_exposed_child_unique_writer.rs` (in `fail/all/`; HB fails one statement earlier than TB — at the wildcard write itself, same reborrow_chain-write-restriction mechanism as `multi_exposed_child` — rather than at the later read) |
| `multi_exposed_siblings_disable` | Two exposed sibling references both get disabled by a third sibling's write; the wildcard access then finds no valid candidate. | `covered` | `hb_multi_exposed_siblings_disable.rs` (in `fail/all/`; confirmed exactly as predicted — sequential RawPtr-source siblings displace each other via the sibling-reborrow rule, so neither exposed tag survives to be found by the wildcard search) |
| `multi_exposed_siblings_foreign` | A wildcard write (through one of two exposed siblings) disables a third, non-exposed sibling foreign to both. | `covered` | `hb_multi_exposed_siblings_foreign.rs` (in `fail/all/`; HB fails one statement earlier than TB — at the wildcard write itself, same sibling-reborrow-displacement mechanism) |
| `multi_exposed_siblings_local` | A wildcard write activates the base allocation; a parent read freezes it (TB "local access" logic); a second wildcard write then fails. | `hb_pass` | `hb_multi_exposed_siblings_local.rs` (in `pass/tb_fail/`; confirmed exactly as predicted — HB's "no freeze on parent read" property means the second write succeeds. A clean, expected divergence, not a new mechanism) |
| `multi_exposed_siblings_unique_writer` | A wildcard write can only be through the mutable exposed sibling (the other is shared/frozen); the write disables the frozen sibling. | `covered` | `hb_multi_exposed_siblings_unique_writer.rs` (in `fail/all/`; HB fails via a different mechanism — the wildcard resolves to the shared sibling's tag, and HB denies the write outright since Read-permission state never grants writes) |
| `protector_conflicted` | Inside a protected closure, a foreign read marks the protected argument as "conflicted"; a wildcard write through it is then UB. | `covered` | `hb_protector_conflicted.rs` (in `fail/all/`; fails, but NOT for the interesting reason — the sibling-reborrow-displacement rule invalidates the argument's tag before the protected call even happens, so the protector-conflict logic this test targets is never actually reached) |
| `protected_wildcard` | A reference derived from a wildcard pointer is passed as a `&mut` parameter and protected; a write to its ancestor is then a foreign write to it. | `hb_pass` | `hb_protected_wildcard.rs` (in `pass/tb_fail/`; **this used to be a compiler crash (ICE), now fixed** — see `compute_retags` in `miri.rs`, fixed 2026-07-01. The protector logic itself works fine when passed a wildcard-derived reference; HB just has no tree-based foreign-write detection for the "write through an unrelated ancestor" pattern, since `ref1` and `wild_ref` never actually displaced each other) |
| `strongly_protected_wildcard` | Deallocating through a raw address (not a concrete tag) while a `&mut` protector is active is UB. | `covered` | `hb_strongly_protected_wildcard.rs` (in `fail/all/`; confirmed cleanly — `before_memory_deallocation` ignores its `ProvenanceExtra` parameter and scans all `BorrowerState` tags for a protector regardless of how the address was derived, so this needs no wildcard-specific handling at all) |
| `protector_release` | Checks that releasing a protector correctly determines a wildcard-rooted tag cannot be its child, disabling it. | `covered` | `hb_protector_release.rs` (in `fail/all/`; fails matching TB's overall verdict, though not via tree-based ancestor/descendant reasoning — a plain tag mismatch on the final read) |
| `protector_release2` | Variant of `protector_release` with a different creation order for the wildcard-rooted reference relative to the protected argument's exposed child. | `covered` | `hb_protector_release2.rs` (in `fail/all/`; same result and mechanism as `protector_release`) |

Remaining affected tests (14): `cross_tree_from_main`,
`cross_tree_update_main_invalid_exposed`, `cross_tree_update_main_invalid_exposed2`,
`cross_tree_update_newer`, `cross_tree_update_newer_exposed`, `cross_tree_update_older`,
`cross_tree_update_older_invalid_exposed`, `cross_tree_update_older_invalid_exposed2`, `dealloc`,
`gc`, `single_exposed_foreign`, `single_exposed_only_ro`,
`subtree_internal_relatedness`, `subtree_internal_relatedness_wildcard`.

---

## Tree Borrows — pass/ tests

| TB test | Description | HB status | HB counterpart |
|---|---|---|---|
| `cell-alternate-writes` | Two aliasing shared references to an `UnsafeCell` remain valid across alternating interleaved writes through each reference. | `covered` | `unsafecell_aliasing_writes.rs` (status corrected from stale `no_std` — a counterpart already existed) |
| `cell-inside-box` | A ReservedIM (`UnsafeCell` inside `Box`) raw pointer survives a foreign write when a sibling pointer transitions to Unique via a child write. | `covered` | `unsafecell_aliasing_writes.rs` (status corrected from stale `no_std` — a counterpart already existed) |
| `cell-inside-struct` | Writing to both a `Cell` field and a non-`Cell` field through a `*mut` cast of a shared `&Foo` is permitted when per-byte interior-mutability precision is disabled. | `covered` | `hb_cell_inside_struct_coarse.rs` (in `pass/all/`; HB has no precise/non-precise interior-mut mode toggle at all — it always runs "coarse", matching this TB variant. The `-no-precise-interior-mut` variant's code is otherwise identical to the always-`no_per_byte` fail/ variant) |
| `cell-lazy-write-to-surrounding` | A raw pointer derived from a shared reference to a `!Freeze` (`Cell`) type retains write permission to surrounding memory under TB. | `no_per_byte` | — |
| `copy-nonoverlapping` | `copy_nonoverlapping` works correctly when a shared pointer is obtained before a mutable pointer into the same allocation (non-overlapping regions). | `covered` | `hb_copy_nonoverlapping.rs` (in `fail/tb_pass/`; `test_to_from` alone passes, but `test_from_to` fails — `data.as_ptr()` leaves a lingering `shared_borrower` via HB's Polonius tracking that conflicts with the later `data.as_mut_ptr()`. HB does not achieve TB's full order-independence; converges with SB's failure on this ordering instead, for an unrelated reason) |
| `end-of-protector` | A mutable-reference protector installed at function entry is fully released when the function returns, allowing a subsequent reborrow to write without UB. | `covered` | `protector_lifetime.rs` |
| `formatting` | Tests pretty-print formatting of TB per-byte tag trees across aliased slice indices with different permission states (Active/Frozen). | `diagnostic_only` | — |
| `read_retag_no_race` | A mutable retag in one thread does not race with a concurrent read in another thread when the non-UB interleaving is enforced deterministically. | `no_concurrency` | — |
| `reborrow-is-read` | Creating a reborrow from a parent mutable reference counts as a Read access, causing a sibling Unique reference to transition to Frozen in TB. | `diagnostic_only` | — |
| `reserved` | Exhaustively checks TB Reserved state transitions (to Frozen, Disabled, or no-op) under all combinations of interior mutability and protector presence. | `covered` | 4 sub-scenarios in `hb_reserved_unprotected.rs` (in `fail/tb_pass/`; **regressed 2026-07-02** — used to pass (4 of 6), now all 4 fail, since the `RawPointerStack.dead` fix kills the never-activated `x` on a sibling reborrow from `base` — see case study); the 2 "protected read" sub-scenarios independently fail — `hb_reserved_protected_conflict.rs` (in `fail/tb_pass/`; `x`/`y` are sequential `&mut *base` reborrows of the same raw pointer, and the second displaces the first's tag per the sibling-reborrow rule added for `hb_wildcard_sibling_disable.rs` — a real, pre-existing HB/TB divergence, unrelated to the `dead`-flag fix) |
| `sb_fails` | Mutable reborrows without actual writes do not invalidate sibling raw pointers (SB fails, TB/HB pass), across four aliasing patterns. | `covered` | `sb_fnentry_doesnt_invalidate_raw.rs` (still passes); `hb_pass_invalid_mut_raw_read.rs`, `hb_return_invalid_mut_raw_read.rs` (in `fail/tb_pass/`; **regressed 2026-07-02** — used to pass, now fail along with SB — see case study); the `static_memory_modification` sub-module is a genuine three-way divergence (SB fail, TB pass, HB fail) — see `hb_static_memory_modification_readonly.rs` in `fail/tb_pass/` |
| `spurious_read` | A spurious read inserted during a protected mutable reborrow is valid when the aliased-pointer write occurs after protector release, using two synchronized threads. | `no_concurrency` | — |
| `transmute-unsafecell` | `mem::transmute` between `&i32` and `&UnsafeCell<i32>` in both directions is valid as long as no write is performed through the Frozen-parent-constrained cell. | `covered` | `hb_transmute_unsafecell.rs` (in `pass/broken/`; HB fails due to PoloniusAnchor firing at transmute site — implementation limitation) |
| `tree-borrows` | TB-specific aliasing patterns: multiple Reserved mut refs coexisting, raw pointer alias with local, protector on disjoint fields, reborrow returned through function boundary. | `covered` | 4 of 6 sub-functions pass — `hb_tree_borrows.rs` (in `pass/all/`); `aliasing_read_only_mutable_refs` genuinely fails — `hb_tree_borrows_sibling_reborrow.rs` (in `fail/tb_pass/`; same sibling-reborrow-displacement mechanism as `hb_reserved_protected_conflict.rs`); `string_as_mut_ptr` not ported (needs `alloc::string::String`, shares `Box`'s allocator machinery, known Polonius-MIR-for-stdlib bug) |

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
