use rustc_abi::Size;
use rustc_data_structures::either::Either;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::{
    Local, PoloniusAnchorData, PoloniusAnchorId, PoloniusAnchorKind, RetagKind,
};
use rustc_middle::ty;
use rustc_middle::ty::layout::HasTypingEnv;

use rustc_data_structures::fx::FxHashMap;

use crate::borrow_tracker::{AccessKind, BorTag, GlobalState, GlobalStateInner, ProtectorKind};
use crate::*;

/// Determine what kind of protector (if any) to install for a FnEntry retag of the given type.
///
/// Mirrors SB's `NewPermission::from_ref_ty` protector logic:
/// - `&mut T` and `&T` → `StrongProtector` (noalias + dereferenceable, dealloc is UB)
/// - `Box<T>`          → `WeakProtector`   (noalias only, dealloc is allowed)
/// - everything else   → `None`
fn hb_protector_for(ty: ty::Ty<'_>) -> Option<ProtectorKind> {
    match ty.kind() {
        ty::Ref(_, _, ty::Mutability::Mut) => Some(ProtectorKind::StrongProtector),
        ty::Ref(_, _, ty::Mutability::Not) => Some(ProtectorKind::StrongProtector),
        ty::Adt(..) if ty.is_box() => Some(ProtectorKind::WeakProtector),
        _ => None,
    }
}

mod borrower;

pub use self::borrower::{BorrowerPermission, BorrowerState, RawPointerStack};

// The Hybrid Borrows alloc-extra is the per-allocation `BorrowerState` directly. SB and TB use
// dedicated wrapper types (`Stacks`, `Tree`); HB stores `BorrowerState` itself, which is why
// `AllocState = BorrowerState` below.

/// Permission requested at a reborrow site.
///
/// Maps to the runtime `BorrowerPermission` transitions inside `hb_reborrow`:
/// - `Read` — building a shared `&T` reborrow.
/// - `Write` — building a mutable `&mut T` reborrow.
/// - `TwoPhase` — building a two-phase mutable borrow; the resulting allocation enters
///   `BorrowerPermission::Reserved` and is later promoted to `Write` by
///   `hb_before_terminator` at the activating call site.
#[derive(Debug)]
pub enum NewPermission {
    Read,
    Write,
    TwoPhase,
}

/// Whether a reborrow's source pointer is a normal reference or a raw pointer.
///
/// Threaded through `hb_retag_ptr_value` → `hb_retag_reference` → `hb_retag_place` →
/// `hb_reborrow` → `apply_reborrow_to_stack`. The `RawPtr` case is what enables the
/// `RawPointerStack` chain to grow; the `Ref` case overwrites the existing top of stack
/// (or `current_borrower` if no stack) without pushing a new entry.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RetagReferenceSource {
    Ref,
    RawPtr,
}

pub type AllocState = BorrowerState;
type BorrowCheckResult = Result<(), String>;

/// Core per-location access logic: validate a tag against `BorrowerState` for a given access kind.
///
/// All four methods in this impl are pure logic over the per-allocation state — they don't touch
/// the Miri context. The two checkers (`check_unique_borrower_tag` and `check_raw_pointer_stack`)
/// implement the two halves of the dispatch, and `check_borrower_tag` chooses between them based
/// on `perms.permission` plus the access kind. `access` is the entry called from
/// `before_memory_access` and converts a `BorrowCheckResult` into `InterpResult`.
impl<'tcx> BorrowerState {
    /// Validate `bor_tag` against `current_borrower` / `prev_borrower` for an access that does
    /// **not** involve the raw-pointer exposed stack.
    ///
    /// Side effect: on a successful match against `current_borrower`, `prev_borrower` is cleared.
    /// (The slot only exists to bridge the brief window after a reborrow when both the parent
    /// and the new tag may legitimately appear at access sites.)
    ///
    /// Status: working. Returns `Ok(())` when `bor_tag` matches `current_borrower` (clearing
    /// any held `prev_borrower`), or matches `prev_borrower`. Otherwise returns a message
    /// describing the mismatch.
    ///
    /// Interacts with: `check_borrower_tag`, `check_raw_pointer_stack` (the alternative path),
    /// `hb_return_mut_borrower` (which writes to `prev_borrower`).
    fn check_unique_borrower_tag(
        &mut self,
        bor_tag: BorTag,
        kind: AccessKind,
        protected: &FxHashMap<BorTag, ProtectorKind>,
    ) -> BorrowCheckResult {
        if self.current_borrower == bor_tag {
            println!(
                "   Access granted: current borrower {:?} matches access tag {:?}",
                self.current_borrower, bor_tag
            );

            // Only clear prev_borrower on a WRITE through the current borrower. A read
            // through current is a "peek" (e.g. parent-read, reborrow validation) and must
            // not evict the child tag stored in prev_borrower — that would permanently kill
            // any raw pointer whose tag was stashed there by the PoloniusAnchor machinery.
            if kind == AccessKind::Write {
                if let Some(prev) = self.prev_borrower {
                    // Before clearing prev_borrower, check whether it is protected.
                    // Clearing a protected tag means it can never be accessed again through
                    // its original handle — that is a protector violation.
                    if let Some(prot_kind) = protected.get(&prev) {
                        return Err(format!(
                            "protector violation: prev_borrower {:?} is {:?}-protected \
                             but displaced by access through {:?}",
                            prev, prot_kind, bor_tag
                        ));
                    }
                    self.prev_borrower = None;
                }
            }
            Ok(())
        } else {
            if let Some(prev_tag) = self.prev_borrower {
                if prev_tag == bor_tag {
                    // Accessing through prev_borrower. Check if the current active borrower
                    // is protected — if so, using the older prev tag is a protector violation
                    // (the protected current tag holds exclusive access).
                    if let Some(prot_kind) = protected.get(&self.current_borrower) {
                        return Err(format!(
                            "protector violation: access via prev_borrower {:?} while \
                             current_borrower {:?} is {:?}-protected",
                            bor_tag, self.current_borrower, prot_kind
                        ));
                    }
                    println!(
                        "   Access granted: previous borrower {:?} matches access tag {:?}",
                        prev_tag, bor_tag
                    );
                    Ok(())
                } else if kind == AccessKind::Read && self.reborrow_chain.contains(&bor_tag) {
                    // READ via an intermediate tag retained in reborrow_chain by the inline-anchor
                    // partial-clear. Raw pointers derived between anchor-restored tags may carry
                    // these intermediate tags; allowing READ preserves their validity.
                    println!(
                        "   Access granted: reborrow_chain READ {:?} (chain: {:?})",
                        bor_tag, self.reborrow_chain
                    );
                    Ok(())
                } else {
                    Err(format!(
                        "Access denied: neither current {:?} nor prev borrower {:?} matches access tag {:?}",
                        self.current_borrower, prev_tag, bor_tag
                    ))
                }
            } else if kind == AccessKind::Read && self.reborrow_chain.contains(&bor_tag) {
                println!(
                    "   Access granted: reborrow_chain READ {:?} (chain: {:?})",
                    bor_tag, self.reborrow_chain
                );
                Ok(())
            } else {
                Err(format!(
                    "Access denied: neither current {:?} nor prev borrower exists for access tag {:?}",
                    self.current_borrower, bor_tag
                ))
            }
        }
    }

    /// Validate `bor_tag` against the raw-pointer exposed stack and `current_borrower`.
    ///
    /// Walks `exposed_stack` from the top down. Two ways to accept:
    /// 1. `bor_tag` matches an entry's `current_borrower` — entries above are popped (siblings
    ///    derived later are invalidated by the access through this older one).
    /// 2. `bor_tag` matches an entry's `base_pointer` — accepts an access through the original
    ///    parent of a raw chain; if the `base_pointer` differs from the lower-level current
    ///    borrower, it's stashed into `prev_borrower` for a possible follow-up access through
    ///    the parent.
    /// Falls back to a direct `current_borrower == bor_tag` check at the bottom of the stack.
    ///
    /// Status: working for the patterns covered by `pass/test1.rs` and `pass/test6.rs`. The
    /// stack truncation logic is the load-bearing piece of raw-pointer aliasing in HB.
    ///
    /// Interacts with: `check_borrower_tag` (the dispatcher), `apply_reborrow_to_stack` (which
    /// pushes entries this method later consumes).
    fn check_raw_pointer_stack(
        &mut self,
        bor_tag: BorTag,
        kind: AccessKind,
        protected: &FxHashMap<BorTag, ProtectorKind>,
    ) -> BorrowCheckResult {
        println!("Checking raw pointer stack for access tag {:?}", bor_tag);
        if let Some(exposed_stack) = self.exposed_stack.as_mut() {
            for idx in (0..exposed_stack.len()).rev() {
                let entry = exposed_stack[idx].clone();
                let lower_current_borrower = if idx > 0 {
                    exposed_stack[idx - 1].current_borrower
                } else {
                    self.current_borrower
                };

                if entry.current_borrower == bor_tag {
                    // Before truncating entries above idx, check whether any of them are
                    // protected. Popping a protected raw-stack entry is a protector violation.
                    for displaced in exposed_stack[idx + 1..].iter() {
                        if let Some(prot_kind) = protected.get(&displaced.current_borrower) {
                            return Err(format!(
                                "protector violation: raw pointer access through {:?} displaced \
                                 protected tag {:?} ({:?})",
                                bor_tag, displaced.current_borrower, prot_kind
                            ));
                        }
                    }
                    exposed_stack.truncate(idx + 1);

                    if exposed_stack.is_empty() {
                        self.exposed_stack = None;
                    }

                    println!(
                        "   Raw pointer stack access {:?}: matched current borrower {:?}, base {:?}, lower borrower {:?}",
                        bor_tag, entry.current_borrower, entry.base_pointer, lower_current_borrower
                    );
                    return Ok(());
                }

                if entry.base_pointer == bor_tag {
                    // A READ through the base pointer is non-destructive: the child borrow
                    // stays live. Only a WRITE reclaims the borrow and kills the child entry.
                    if kind == AccessKind::Read {
                        // StrongProtectors fire on any foreign access, including reads.
                        // A WeakProtector only fires on writes, so reads are fine.
                        if let Some(ProtectorKind::StrongProtector) = protected.get(&entry.current_borrower) {
                            return Err(format!(
                                "protector violation: read through base pointer {:?} while \
                                 strong-protected child {:?} is active",
                                bor_tag, entry.current_borrower
                            ));
                        }
                        println!(
                            "   Raw pointer stack access {:?}: base pointer READ, child {:?} preserved",
                            bor_tag, entry.current_borrower
                        );
                        return Ok(());
                    }

                    // WRITE: reclaim the borrow — check protectors first.
                    for displaced in exposed_stack[idx + 1..].iter() {
                        if let Some(prot_kind) = protected.get(&displaced.current_borrower) {
                            return Err(format!(
                                "protector violation: raw pointer access through {:?} displaced \
                                 protected tag {:?} ({:?})",
                                bor_tag, displaced.current_borrower, prot_kind
                            ));
                        }
                    }
                    // Also check the entry at idx itself if it will be popped.
                    if entry.base_pointer != lower_current_borrower {
                        if let Some(prot_kind) = protected.get(&entry.current_borrower) {
                            return Err(format!(
                                "protector violation: raw pointer base access through {:?} displaced \
                                 protected tag {:?} ({:?})",
                                bor_tag, entry.current_borrower, prot_kind
                            ));
                        }
                    }

                    exposed_stack.truncate(idx + 1);

                    if entry.base_pointer != lower_current_borrower {
                        self.prev_borrower = Some(entry.base_pointer);
                        exposed_stack.pop();
                    }

                    if exposed_stack.is_empty() {
                        self.exposed_stack = None;
                    }

                    println!(
                        "   Raw pointer stack access {:?}: base pointer WRITE, reclaimed from {:?}, lower borrower {:?}",
                        bor_tag, entry.current_borrower, lower_current_borrower
                    );
                    return Ok(());
                }
            }
        }

        if self.current_borrower == bor_tag {
            println!(
                "   Raw pointer stack access fell back to current borrower {:?}",
                self.current_borrower
            );
            return Ok(());
        }

        Err(format!(
            "Access denied: no raw pointer stack or current borrower match for access tag {:?}",
            bor_tag
        ))
    }

    /// Top-level access check: validate `bor_tag` against the current `BorrowerState` for an
    /// access of `kind`. The dispatcher of the access path.
    ///
    /// `protected` is the global protected-tags map, threaded in from `access()` so the inner
    /// checkers can verify that no protected tag is displaced as a side-effect of this access.
    ///
    /// Behavior depends on `perms.permission`:
    /// - `Read` — read accepted via `shared_borrower` tag, otherwise via the unique-borrower /
    ///   raw-stack path. Writes are denied.
    /// - `Write` — uses raw stack if present, otherwise unique borrower.
    /// - `Frozen` — read accepted via `shared_borrower` or unique borrower; a write upgrades
    ///   the location to `Write` permission and clears `shared_borrower`.
    /// - `Reserved` (two-phase) — read accepted via either the reserving tag or the
    ///   `shared_borrower`; a write activates: permission becomes `Write` and `shared_borrower`
    ///   is cleared. (The activation that fires at the call site is in `hb_before_terminator`;
    ///   this is the access-time activation.)
    ///
    /// Interacts with: `check_unique_borrower_tag`, `check_raw_pointer_stack`, `access` (caller).
    fn check_borrower_tag(
        &mut self,
        bor_tag: BorTag,
        kind: AccessKind,
        protected: &FxHashMap<BorTag, ProtectorKind>,
    ) -> BorrowCheckResult {
        match self.perms.permission {
            BorrowerPermission::Read => {
                match kind {
                    AccessKind::Read => {
                        let (shr_tag, _count) = self.shared_borrower.unwrap();
                        if shr_tag == bor_tag {
                            Ok(())
                        } else {
                            if self.exposed_stack.is_some() {
                                self.check_raw_pointer_stack(bor_tag, kind, protected)
                            } else {
                                self.check_unique_borrower_tag(bor_tag, kind, protected)
                            }
                        }
                    }
                    AccessKind::Write =>
                        Err(format!(
                            "Access denied: write access on a shared borrow with tag {:?}",
                            bor_tag
                        )),
                }
            }
            BorrowerPermission::Write =>
                if self.exposed_stack.is_some() {
                    self.check_raw_pointer_stack(bor_tag, kind, protected)
                } else {
                    self.check_unique_borrower_tag(bor_tag, kind, protected)
                },
            BorrowerPermission::Frozen =>
                match kind {
                    AccessKind::Read => {
                        let (shr_tag, _count) = self.shared_borrower.unwrap();
                        if shr_tag == bor_tag {
                            Ok(())
                        } else {
                            self.check_unique_borrower_tag(bor_tag, kind, protected)
                        }
                    }
                    AccessKind::Write => {
                        self.check_unique_borrower_tag(bor_tag, kind, protected)?;
                        self.perms.permission = BorrowerPermission::Write;
                        self.shared_borrower = None;
                        Ok(())
                    }
                },
            BorrowerPermission::Reserved =>
                match kind {
                    AccessKind::Read => {
                        if self.check_unique_borrower_tag(bor_tag, kind, protected).is_ok() {
                            Ok(())
                        } else {
                            let (shr_tag, _count) = self.shared_borrower.unwrap();
                            if shr_tag == bor_tag {
                                println!("Access Allowed: {:?}", shr_tag);
                                Ok(())
                            } else {
                                Err(format!(
                                    "Access not allowed: bor tag {:?} and shr tag {:?}",
                                    bor_tag, shr_tag
                                ))
                            }
                        }
                    }
                    AccessKind::Write => {
                        // Only the reserved tag itself (current_borrower) may activate by writing.
                        // Any write via a different tag (including prev_borrower) is a foreign
                        // write that Freeze-type Reserved does not tolerate.
                        if self.current_borrower != bor_tag {
                            return Err(format!(
                                "Access denied: foreign write via {:?} during Reserved borrow \
                                 (current_borrower = {:?}); only the reserved tag may activate",
                                bor_tag, self.current_borrower
                            ));
                        }
                        if let Some(prev) = self.prev_borrower {
                            if let Some(prot_kind) = protected.get(&prev) {
                                return Err(format!(
                                    "protector violation: prev_borrower {:?} is {:?}-protected \
                                     but displaced by Reserved activation via {:?}",
                                    prev, prot_kind, bor_tag
                                ));
                            }
                            self.prev_borrower = None;
                        }
                        self.perms.permission = BorrowerPermission::Write;
                        self.shared_borrower = None;
                        Ok(())
                    }
                },
            // Two-phase borrow over a !Freeze type: tolerates foreign writes via shared_borrower.
            BorrowerPermission::ReservedIM =>
                match kind {
                    AccessKind::Read => {
                        // Identical to Reserved::Read: accept current_borrower or shared_borrower.
                        if self.check_unique_borrower_tag(bor_tag, kind, protected).is_ok() {
                            Ok(())
                        } else {
                            let (shr_tag, _count) = self.shared_borrower.unwrap();
                            if shr_tag == bor_tag {
                                Ok(())
                            } else {
                                Err(format!(
                                    "ReservedIM: read denied for tag {:?} (current={:?}, shared={:?})",
                                    bor_tag, self.current_borrower, shr_tag
                                ))
                            }
                        }
                    }
                    AccessKind::Write => {
                        if self.check_unique_borrower_tag(bor_tag, kind, protected).is_ok() {
                            // current_borrower wrote → activation, same as Reserved.
                            self.perms.permission = BorrowerPermission::Write;
                            self.shared_borrower = None;
                            Ok(())
                        } else if self.shared_borrower.map(|(t, _)| t) == Some(bor_tag) {
                            // shared_borrower (T0) wrote → foreign write tolerated, stay ReservedIM.
                            println!(
                                "ReservedIM: tolerated foreign write via shared_borrower {:?}",
                                bor_tag
                            );
                            Ok(())
                        } else {
                            Err(format!(
                                "ReservedIM: write denied for tag {:?} (neither current_borrower {:?} nor shared_borrower {:?})",
                                bor_tag, self.current_borrower, self.shared_borrower
                            ))
                        }
                    }
                },
        }
    }

    /// Entry point invoked from `before_memory_access` for each memory read or write.
    ///
    /// Extracts `protected_tags` from the machine's global borrow-tracker state and threads it
    /// through `check_borrower_tag` so displacement checks can consult it.
    ///
    /// Status: partial. The non-concrete branch is an explicit TODO — without it, accesses
    /// through wildcard provenance silently bypass aliasing checks.
    ///
    /// Interacts with: `before_memory_access` (caller), `check_borrower_tag` (delegate).
    fn access(
        &mut self,
        tag: ProvenanceExtra,
        kind: AccessKind,
        machine: &MiriMachine<'_>,
    ) -> InterpResult<'tcx> {
        if let ProvenanceExtra::Concrete(bor_tag) = tag {
            println!(
                "Access with concrete tag {:?}, kind {:?}, and current permission {:?}",
                bor_tag, kind, self.perms.permission
            );
            let protected = machine
                .borrow_tracker
                .as_ref()
                .map(|bt| bt.borrow())
                .expect("borrow tracker must be active");
            self.check_borrower_tag(bor_tag, kind, &protected.protected_tags)
                .map_err(|msg| err_ub_format!("{msg}"))?;
        } else {
            // TODO: will handle case where tag is not concrete
            println!("   Access with non-concrete access tag {:?}", tag);
        }
        interp_ok(())
    }
}

/// Allocation-lifecycle hooks invoked by Miri's alloc-extra system.
///
/// These are the callbacks that route Miri events — allocation creation, memory access,
/// deallocation, GC sweeps, frame-exit protector release — into per-allocation
/// `BorrowerState` updates. Dispatch from outside arrives via
/// `borrow_tracker::AllocState::HybridBorrows`.
impl BorrowerState {
    /// Build a fresh `BorrowerState` for a newly-created allocation.
    ///
    /// Acquires the allocation's root pointer tag from the global state and seeds the
    /// `BorrowerState` with it as `current_borrower` (in `Write` permission, no prev/shared,
    /// no exposed stack — see `BorrowerState::new` in `borrower.rs`).
    ///
    /// Status: working.
    ///
    /// Interacts with: `GlobalStateInner::root_ptr_tag` (in `borrow_tracker/mod.rs`).
    pub fn new_allocation(
        id: AllocId,
        _alloc_size: Size,
        state: &mut GlobalStateInner,
        _kind: MemoryKind,
        machine: &MiriMachine<'_>,
    ) -> Self {
        let tag = state.root_ptr_tag(id, machine);
        BorrowerState::new(tag)
    }

    /// Hook invoked on every memory read or write touching this allocation.
    ///
    /// Delegates to the `access` access-path entry. The `_alloc_id`, `_range`, and `_machine`
    /// parameters are presently unused; they're kept in the signature to match the dispatch
    /// interface and to leave room for protector-aware diagnostics.
    ///
    /// Status: working for the corpus.
    ///
    /// Interacts with: `BorrowerState::access`.
    pub fn before_memory_access<'tcx>(
        &mut self,
        kind: AccessKind,
        _alloc_id: AllocId,
        tag: ProvenanceExtra,
        _range: AllocRange,
        _machine: &MiriMachine<'tcx>,
    ) -> InterpResult<'tcx> {
        //let location = machine.threads.active_thread_stack().last().map(|frame| frame.current_loc());

        self.access(tag, kind, _machine)?;

        interp_ok(())
    }

    /// Hook invoked just before this allocation is deallocated.
    ///
    /// Status: **not implemented** — empty body. SB and TB use this hook to enforce the
    /// "no deallocation while a `StrongProtector` exists on the allocation" rule. HB has no
    /// protectors yet, so this is currently a no-op. Implementing protectors will require
    /// scanning `GlobalStateInner.protected_tags` here for any `StrongProtector` whose tag
    /// belongs to this allocation, and erroring if so. See [docs/open-work.md § Protectors].
    ///
    /// Interacts with: future protector enforcement in Phase 2 of the protector plan.
    pub fn before_memory_deallocation<'tcx>(
        &mut self,
        alloc_id: AllocId,
        _prov_extra: ProvenanceExtra,
        _size: Size,
        machine: &MiriMachine<'tcx>,
    ) -> InterpResult<'tcx> {
        // StrongProtectors forbid deallocation; WeakProtectors (Box) allow it.
        // Check whether current_borrower is strongly protected.
        let protected_tags = machine
            .borrow_tracker
            .as_ref()
            .map(|bt| bt.borrow())
            .expect("borrow tracker must be active");
        if let Some(ProtectorKind::StrongProtector) =
            protected_tags.protected_tags.get(&self.current_borrower)
        {
            throw_ub_format!(
                "deallocating alloc {:?} while tag {:?} is strongly protected",
                alloc_id,
                self.current_borrower
            );
        }
        interp_ok(())
    }

    /// Apply a mutable reborrow to the exposed-stack state, handling both `Ref` and `RawPtr`
    /// sources. Encapsulates the "where does the new tag go in the stack?" logic that drives
    /// `Write → Write` and `Write → TwoPhase` reborrows in `hb_reborrow`.
    ///
    /// Returns the *previous effective borrower tag* — top-of-stack `current_borrower` when a
    /// stack exists, otherwise `self.current_borrower`. Callers use this for the
    /// `shared_borrower` entry when creating a two-phase (Reserved) borrow.
    ///
    /// `RawPtr` source: pushes a new `RawPointerStack` entry rooted at the old top, recovers
    /// via `prev_borrower` when no stack is present, or updates the stack top in place when
    /// the existing top's `base_pointer` matches.
    ///
    /// `Ref` source: overwrites the stack top's `current_borrower` (if a stack exists) or
    /// `self.current_borrower` directly. No new stack entry is created.
    ///
    /// # Errors
    /// Returns a string describing UB if the reborrow is not permitted (e.g. the parent tag
    /// does not match the effective current borrower and recovery via `prev_borrower` /
    /// stack `base_pointer` fails).
    ///
    /// Status: working — this is the load-bearing piece of HB's raw-pointer-derived borrow
    /// support and is exercised by `pass/test1.rs`, `pass/test6.rs`, and the matching `fail/`
    /// cases.
    ///
    /// Interacts with: `hb_reborrow` (caller), `check_raw_pointer_stack` (consumer of pushed
    /// entries).
    pub fn apply_reborrow_to_stack(
        &mut self,
        parent_tag: BorTag,
        new_tag: BorTag,
        source: RetagReferenceSource,
        alloc_id: AllocId,
    ) -> Result<BorTag, String> {
        // The "effective current borrower" is the top of the exposed stack when one exists,
        // otherwise it is the bare current_borrower field.
        let old_tag = self
            .exposed_stack
            .as_deref()
            .and_then(<[_]>::last)
            .map(|e| e.current_borrower)
            .unwrap_or(self.current_borrower);

        match source {
            RetagReferenceSource::RawPtr => {
                if old_tag == parent_tag {
                    // Parent matches the effective current borrower: push a new stack entry that
                    // records the old tag as the base and the new tag as the current borrower.
                    self.exposed_stack
                        .get_or_insert_with(Vec::new)
                        .push(RawPointerStack { base_pointer: old_tag, current_borrower: new_tag });
                    println!(
                        "Updated allocation {:?} exposed_stack to {:?}",
                        alloc_id, self.exposed_stack
                    );
                } else {
                    // Parent does not match the effective current borrower — try to recover.
                    let stack_non_empty =
                        self.exposed_stack.as_ref().map_or(false, |s| !s.is_empty());
                    if stack_non_empty {
                        // Stack present: the top entry's base_pointer must equal old_tag for the
                        // reborrow to make sense (the entry is in a "self-loop" state where
                        // base == current).  If so, update its current_borrower in-place.
                        let stack = self.exposed_stack.as_mut().unwrap();
                        let base_pointer = stack.last().unwrap().base_pointer;
                        if old_tag != base_pointer {
                            return Err(format!(
                                "Raw pointer reborrow with tag {:?} does not match base pointer \
                                 {:?} for allocation {:?}",
                                parent_tag, base_pointer, alloc_id
                            ));
                        }
                        stack.last_mut().unwrap().current_borrower = new_tag;
                    } else {
                        // No stack: check if prev_borrower matches — if so we can start a new
                        // stack entry rooted at prev_borrower.
                        match self.prev_borrower {
                            Some(prev_tag) if prev_tag == parent_tag => {
                                self.exposed_stack
                                    .get_or_insert_with(Vec::new)
                                    .push(RawPointerStack {
                                        base_pointer: prev_tag,
                                        current_borrower: new_tag,
                                    });
                                println!(
                                    "Updated allocation {:?} exposed_stack to {:?}",
                                    alloc_id, self.exposed_stack
                                );
                            }
                            _ =>
                                return Err(format!(
                                    "Raw pointer reborrow with tag {:?} does not match current \
                                     borrower {:?}, prev borrower {:?} and has no exposed stack \
                                     for allocation {:?}",
                                    parent_tag,
                                    self.current_borrower,
                                    self.prev_borrower,
                                    alloc_id
                                )),
                        }
                    }
                }
            }
            RetagReferenceSource::Ref => {
                println!("exposed stack is {:?}", self.exposed_stack);
                if let Some(stack) = self.exposed_stack.as_mut() {
                    if let Some(top) = stack.last_mut() {
                        top.current_borrower = new_tag;
                        println!(
                            "Updated allocation {:?} current_borrower to {:?} in Stack",
                            alloc_id, new_tag
                        );
                    }
                } else {
                    // Record the displaced tag so `release_protector` can verify that the
                    // new effective borrower is a legitimate child, not an alias.
                    self.reborrow_chain.push(self.current_borrower);
                    self.current_borrower = new_tag;
                    println!(
                        "Updated allocation {:?} current_borrower to {:?} (reborrow_chain: {:?})",
                        alloc_id, new_tag, self.reborrow_chain
                    );
                }
            }
        }

        Ok(old_tag)
    }

    /// GC hook: called with the set of tags currently judged unreachable by Miri's provenance GC.
    ///
    /// SB and TB use this to drop now-dead tags from their internal stacks/trees so the
    /// per-allocation state doesn't accumulate forever. HB's `BorrowerState` only stores a
    /// fixed-size slice of tags (current/prev/shared plus exposed_stack entries), so there's
    /// less to clean up — but `prev_borrower`, `shared_borrower`, and stack tags can in
    /// principle be cleared here.
    ///
    /// Status: not implemented — empty body. Combined with `visit_provenance` only walking
    /// `current_borrower`, this means GC behavior on HB allocations is approximate. See
    /// [status.md] entries on GC and `remove_unreachable_tags`.
    pub fn remove_unreachable_tags(&self, _tags: &FxHashSet<BorTag>) {}

    /// Frame-exit hook: called by the generic `on_stack_pop` in `borrow_tracker/mod.rs` once per
    /// `(AllocId, BorTag)` pair recorded in the popped frame's `protected_tags` list.
    ///
    /// SB and TB use this to (a) optionally fire an implicit read through the protected tag
    /// (for `StrongProtector`, to detect stale memory), and (b) remove the tag from the
    /// per-allocation protection set. The generic `end_call` then removes it from the global
    /// `protected_tags` map.
    ///
    /// Status: **not implemented** — logging-only no-op. Phase 3 of the protector plan
    /// ([docs/open-work.md § Protectors]) replaces this body with the real implementation.
    /// Until then, no `(alloc_id, tag)` pairs are ever pushed into HB's frames, so this is
    /// effectively dead code.
    ///
    /// Interacts with: future Phase-1 work that will push to `frame.extra.protected_tags`.
    pub fn release_protector<'tcx>(
        &self,
        _machine: &MiriMachine<'tcx>,
        global: &GlobalState,
        tag: BorTag,
        alloc_id: AllocId,
    ) -> InterpResult<'tcx> {
        let kind = global.borrow().protected_tags.get(&tag).copied();
        match kind {
            Some(ProtectorKind::StrongProtector) => {
                // Implicit read: the protected tag must still be the *effective* current
                // borrower, OR the effective borrower must be a descendant of the protected
                // tag via a Ref-source reborrow chain (`reborrow_chain`).
                //
                // The second condition handles the common pattern where the function body
                // creates a child reborrow of its own argument (e.g. `Option::as_mut` does
                // `Some(ref mut x)` which produces a `&mut T` child of `self`). In Stacked
                // Borrows the parent tag stays in the borrow stack below the child; in HB
                // we record it in `reborrow_chain` instead.
                //
                // When the function argument was raw-pointer-derived, the tag lives in
                // `exposed_stack.last().current_borrower` rather than `current_borrower`. If
                // something corrupted the allocation (e.g. a write through the raw base
                // pointer), the stack entry gets popped and the effective borrower will differ
                // from `tag` — and `reborrow_chain` will not contain `tag` either, so the
                // violation is still caught.
                let effective = self
                    .exposed_stack
                    .as_deref()
                    .and_then(<[_]>::last)
                    .map(|e| e.current_borrower)
                    .unwrap_or(self.current_borrower);
                // The tag is still "reachable" if it is:
                //   1. The effective top-of-stack current borrower, OR
                //   2. In the Ref-reborrow chain (child reborrow created inside the function), OR
                //   3. A base_pointer or current_borrower anywhere in the exposed_stack —
                //      this covers the pattern where foo(x: &mut T) creates a raw-ptr reborrow
                //      of x and returns it: T_fnentry becomes the exposed_stack base_pointer, and
                //      the stack's current_borrower advances to the returned child. T_fnentry is
                //      still the root of the raw-ptr chain, so the allocation is not invalidated.
                let in_stack = self.exposed_stack.as_deref().map_or(false, |stack| {
                    stack.iter().any(|e| e.base_pointer == tag || e.current_borrower == tag)
                });
                if effective != tag && !self.reborrow_chain.contains(&tag) && !in_stack {
                    throw_ub_format!(
                        "protector violation at frame exit: \
                         tag {:?} in alloc {:?} is no longer the effective current borrower \
                         (effective is {:?}, base current_borrower is {:?}) — \
                         memory was likely invalidated via an aliasing raw pointer during the call",
                        tag,
                        alloc_id,
                        effective,
                        self.current_borrower
                    );
                }
                println!(
                    "[HB] release_protector: implicit read OK for tag {:?} in alloc {:?} \
                     (effective={:?}, in_chain={}, in_stack={})",
                    tag, alloc_id, effective, self.reborrow_chain.contains(&tag), in_stack
                );
            }
            Some(ProtectorKind::WeakProtector) => {
                // WeakProtector (Box): deallocation is permitted, so no implicit read.
                // Just let end_call remove the tag from the global map.
                println!(
                    "[HB] release_protector: weak protector released for tag {:?} in alloc {:?}",
                    tag, alloc_id
                );
            }
            None => {
                // Tag was already released early (e.g. Polonius early release). Nothing to do.
            }
        }
        interp_ok(())
    }
}

/// GC support: tells Miri's provenance GC which tags this `BorrowerState` keeps alive.
///
/// Status: partial. Only visits `current_borrower`. Misses `prev_borrower`, the
/// `shared_borrower` tag, and every tag in `exposed_stack`. In practice this is mostly safe
/// because those tags also survive elsewhere (e.g. on locals, in `FrameState.protected_tags`),
/// but it's a known soundness wart for the GC. See [status.md] "Provenance GC visit".
impl VisitProvenance for BorrowerState {
    /// Visit the `current_borrower` tag only. See the `impl` doc comment above for the gap.
    fn visit_provenance(&self, visit: &mut VisitWith<'_>) {
        visit(None, Some(self.current_borrower));
    }
}

impl<'tcx> EvalContextPrivExt<'tcx> for crate::MiriInterpCx<'tcx> {}
/// Private helpers used by the public `hb_*` surface in `EvalContextExt`.
///
/// Everything in this trait runs against a `MiriInterpCx` (so it has access to allocations,
/// frames, MIR bodies, Polonius facts) but is not directly dispatched from the generic
/// `borrow_tracker/mod.rs` interface.
trait EvalContextPrivExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    /// Reinstall a previously-recorded mutable borrow's tag as the `current_borrower` of its
    /// allocation, when Polonius signals that an outstanding loan has expired and the parent
    /// reference should "take back" access.
    ///
    /// Given a `Local` that holds (or holds a reference to) the borrowed allocation, looks up
    /// the underlying `(alloc_id, tag)` and feeds the tag into `hb_return_mut_borrower`.
    ///
    /// Status: **partial / known-buggy**. The inline doc comment in the body documents the
    /// issue: `compute_return_borrowers` currently passes the *base* local (e.g. `_1` in
    /// `_8 = &mut _1`) rather than the reference local (`_8`). Because creating `_8` already
    /// updated the allocation's `current_borrower` to `_8`'s tag, this lookup recovers the
    /// *newest* tag rather than the borrower we wanted to restore. Several "loan returned"
    /// cases therefore become silent no-ops. Tracked in [docs/open-work.md § ReturnBorrowers
    /// correctness].
    ///
    /// Interacts with: `hb_return_mut_borrower` (delegate), `hb_apply_return_borrowers`
    /// (caller via Polonius anchors), `ReturnBorrowers` in `machine.rs`.
    fn hb_restore_return_borrower_from_local(
        &mut self,
        local: Local,
        preserve_prev: bool,
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();
        let src = this.local_to_place(local)?;

        // `compute_return_borrowers` currently passes the borrowed base local here.
        // For `_8 = &mut _1`, this code receives `_1`, not `_8`.
        //
        // That means the tag we extract below comes from the current state of `_1`'s
        // allocation. But creating `_8` already updated that allocation's
        // `current_borrower` to `_8`'s tag, so this lookup usually recovers the *newest*
        // tag rather than the borrower we wanted to restore after `_8` died.
        let (alloc_id, _, tag) = if src.layout.ty.is_ref() {
            let imm = this.read_immediate(&src)?;
            let mplace = this.ref_to_mplace(&imm)?;
            this.ptr_get_alloc_id(mplace.ptr(), 0)?
        } else {
            let mplace = this.force_allocation(&src)?;
            this.ptr_get_alloc_id(mplace.ptr(), 0)?
        };

        if let ProvenanceExtra::Concrete(bor_tag) = tag {
            this.hb_return_mut_borrower(alloc_id, bor_tag, preserve_prev)?;
            println!("Updated return allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
        }

        interp_ok(())
    }

    /// Handle a Polonius signal that a shared-reference variable has gone out of scope.
    ///
    /// Decrements the `shared_borrower` refcount on the underlying allocation. When the count
    /// reaches zero and the location is still in `Read` permission, transitions to `Frozen`
    /// (which means the location remembers the most-recent shared tag and a subsequent write
    /// can upgrade back to `Write`).
    ///
    /// Status: working for the corpus.
    ///
    /// Interacts with: `hb_apply_return_borrowers` (caller), `hb_handle_polonius_anchor`'s
    /// `SharedReturnVar` branch, `BorrowerPermission::{Read, Frozen}`.
    fn hb_handle_dropped_shared_return_var_from_local(
        &mut self,
        local: Local,
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();
        let src = this.local_to_place(local)?;

        let (alloc_id, _, _tag) = if src.layout.ty.is_ref() {
            let imm = this.read_immediate(&src)?;
            let mplace = this.ref_to_mplace(&imm)?;
            this.ptr_get_alloc_id(mplace.ptr(), 0)?
        } else {
            let mplace = this.force_allocation(&src)?;
            this.ptr_get_alloc_id(mplace.ptr(), 0)?
        };

        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state =
                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
            if borrower_state.perms.permission == BorrowerPermission::Read {
                let shared_borrower_info = borrower_state.shared_borrower.as_mut().unwrap();
                shared_borrower_info.1 -= 1;
                println!(
                    "Updated allocation {:?} with {:?} shared refs",
                    alloc_id, shared_borrower_info.1
                );
                if shared_borrower_info.1 == 0 {
                    borrower_state.perms.permission = BorrowerPermission::Frozen;
                    println!("Shared borrower for allocation {:?} is now frozen", alloc_id);
                }
            }
        }

        interp_ok(())
    }

    /// At a control-flow location with associated `ReturnBorrowers` facts, apply each kind of
    /// loan-return to the runtime state.
    ///
    /// Three groups are processed (in this order):
    /// 1. `return_shared_var` — shared loans whose owning variable has died; routed to
    ///    `hb_handle_dropped_shared_return_var_from_local`.
    /// 2. `mut_borrows` — mutable loans being returned; routed to
    ///    `hb_restore_return_borrower_from_local` to reinstall the parent's tag.
    /// 3. `two_phase` — same shape as `mut_borrows` for two-phase loans.
    ///
    /// `return_ref_args` and the legacy `shared` field on `ReturnBorrowers` are not consumed
    /// here; the former is logged at `Return` terminators in `hb_before_terminator` only.
    ///
    /// Status: works for the patterns it covers, but inherits the
    /// `hb_restore_return_borrower_from_local` correctness issue — see [docs/open-work.md
    /// § ReturnBorrowers correctness].
    ///
    /// Interacts with: `hb_before_statement` (caller via `predecessor_borrowers`),
    /// `hb_handle_polonius_anchor`, `ReturnBorrowers` (input data shape).
    fn hb_apply_return_borrowers(
        &mut self,
        target_loc: rustc_middle::mir::Location,
        return_borrowers: crate::ReturnBorrowers,
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        let mut_locals = return_borrowers.mut_borrows.clone();
        let two_phase_locals = return_borrowers.two_phase.clone();
        let dropped_shared_vars = return_borrowers.return_shared_var.clone();

        if let Some(dropped_shared_vars) = dropped_shared_vars {
            println!(
                "Found dropped shared reference vars at {:?}: {:?}",
                target_loc, dropped_shared_vars,
            );
            for local in dropped_shared_vars {
                this.hb_handle_dropped_shared_return_var_from_local(local)?;
            }
        }

        if let Some(mut_locals) = mut_locals {
            println!("Found return mut borrowers at {:?}: {:?}", target_loc, mut_locals);

            for local in mut_locals {
                this.hb_restore_return_borrower_from_local(local, true)?;
            }
        }

        if let Some(two_phase_locals) = two_phase_locals {
            println!(
                "Found return two-phase borrowers at {:?}: {:?}",
                target_loc, two_phase_locals,
            );

            for local in two_phase_locals {
                this.hb_restore_return_borrower_from_local(local, true)?;
            }
        }

        interp_ok(())
    }

    /// Set `new_tag` as `current_borrower` of `alloc_id` and stash the old tag into
    /// `prev_borrower`, modeling the "parent reference takes back its loan" transition.
    ///
    /// Special case: if the location is currently in `Read` permission, the location is
    /// transitioned to `Frozen` (since reinstalling a mutable parent over a shared loan
    /// effectively closes the shared phase but doesn't yet upgrade to exclusive write).
    ///
    /// Only operates on `LiveData` allocations.
    ///
    /// Status: working as an isolated update primitive — but its caller
    /// `hb_restore_return_borrower_from_local` may be feeding it the wrong tag (see that
    /// function's status note and [docs/open-work.md § ReturnBorrowers correctness]).
    ///
    /// Interacts with: `hb_restore_return_borrower_from_local` (caller),
    /// `BorrowerPermission::{Read, Frozen, Write}`.
    fn hb_return_mut_borrower(
        &mut self,
        alloc_id: AllocId,
        new_tag: BorTag,
        // When true (function-call return): only write prev_borrower if the slot is currently
        // empty — a raw pointer's tag stashed there by an inline PoloniusAnchor must not be
        // overwritten by the now-dead callee tag (e.g. T_fnentry).
        // When false (inline PoloniusAnchor): always overwrite prev_borrower so that scope-end
        // anchors correctly chain the borrow back to the parent for subsequent writes.
        preserve_prev: bool,
    ) -> InterpResult<'tcx> {
        // this return borrower should be called only when it is returning a mut borrow. Therefore, we do need to update state directly
        let this = self.eval_context_mut();

        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state =
                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
            let old_tag = borrower_state.current_borrower;
            borrower_state.current_borrower = new_tag;
            if preserve_prev {
                if borrower_state.prev_borrower.is_none() {
                    borrower_state.prev_borrower = Some(old_tag);
                }
            } else {
                borrower_state.prev_borrower = Some(old_tag);
                // Keep intermediate chain tags — those displaced by Ref-source reborrows
                // between new_tag and old_tag. A raw pointer may carry one of these
                // intermediate tags; retaining it allows READ access via that pointer after
                // the anchor restores new_tag as current. Tags equal to new_tag or old_tag
                // are already reachable via current/prev and do not need a chain entry.
                borrower_state.reborrow_chain.retain(|&t| t != new_tag && t != old_tag);
                return interp_ok(());
            }
            borrower_state.reborrow_chain.clear();

            // if it is in 'read state', we move it to frozen directly.
            if borrower_state.perms.permission == BorrowerPermission::Read {
                //borrower_state.shared_borrower = None;
                borrower_state.perms.permission = BorrowerPermission::Frozen;
            }
        }

        interp_ok(())
    }

    /// Core reborrow logic: validate the parent tag and update `BorrowerState` for the new tag.
    ///
    /// The transition table is keyed by the pair `(current BorrowerPermission, requested
    /// NewPermission)`:
    ///
    /// | from / to    | Read                          | Write                                                        | TwoPhase                                                            |
    /// |--------------|-------------------------------|--------------------------------------------------------------|---------------------------------------------------------------------|
    /// | `Write`      | enter `Read`, set shared tag  | `apply_reborrow_to_stack` (push raw entry or overwrite top)  | `apply_reborrow_to_stack` + Reserved permission, shared = old tag    |
    /// | `Read`       | refcount bump, inherit tag    | UB                                                           | UB                                                                  |
    /// | `Frozen`     | inherit shared tag            | upgrade to `Write`, set new tag as borrower                  | (handled by direct field update + Reserved + shared = old)          |
    /// | `Reserved`   | inherit shared tag            | promote to `Write`                                           | start a fresh two-phase, shared = old tag                           |
    ///
    /// Returns the new `Provenance` to write back into the place. Errors are reported as
    /// `err_ub_format!` strings.
    ///
    /// Status: working for the corpus. The protector side-effect that should fire on
    /// `RetagKind::FnEntry` is **not** present here; see [docs/open-work.md § Protectors,
    /// Phase 1].
    ///
    /// Interacts with: `apply_reborrow_to_stack` (Write/TwoPhase from Write), `check_borrower_tag`
    /// (parent-tag validation), `hb_retag_place` (caller).
    fn hb_reborrow(
        &mut self,
        place: &MPlaceTy<'tcx>,
        perm: NewPermission,
        new_tag: BorTag,
        source: RetagReferenceSource,
    ) -> InterpResult<'tcx, Option<Provenance>> {
        let this = self.eval_context_mut();
        let _ = source;

        // Get allocation info from the place pointer
        let Ok((alloc_id, _base_offset, parent_prov)) = this.ptr_try_get_alloc_id(place.ptr(), 0)
        else {
            // No allocation, just keep original provenance
            return interp_ok(place.ptr().provenance);
        };

        // we do need to check if this 'parent_prov' is legit the 'current borrower' or not.
        let ProvenanceExtra::Concrete(parent_tag) = parent_prov else {
            // If the parent provenance is not concrete, we can't track it, so we just return the original provenance
            return interp_ok(place.ptr().provenance);
        };

        let mut new_prov = Provenance::Concrete { alloc_id, tag: new_tag };

        // Check if the pointee type contains UnsafeCell (i.e., is !Freeze).
        // Must be computed before borrower_state is taken (split-borrow safety).
        let ty_is_freeze = place.layout.ty.is_freeze(*this.tcx, this.typing_env());

        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state =
                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();

            // Validate the parent tag before creating the child reborrow.
            // We pass an empty protected map here because this check is purely validating
            // that the parent tag is currently valid — it is not an access that displaces
            // any other tag, so no protector check is needed at this call site.
            let empty_protected = FxHashMap::default();
            borrower_state
                .check_borrower_tag(parent_tag, AccessKind::Read, &empty_protected)
                .map_err(|msg| {
                    err_ub_format!(
                        "invalid parent tag {:?} for reborrow at allocation {:?} with permission {:?}: {}",
                        parent_tag,
                        alloc_id,
                        borrower_state.perms.permission,
                        msg
                    )
                })?;

            // Phase 1: !Freeze shared reborrows inherit the parent tag.
            // The allocation stays in Write state; no shared_borrower transition occurs.
            // Multiple &UnsafeCell<T> refs all carry parent_tag (= current_borrower) and
            // have write authority through the normal Write check — no new state needed.
            if matches!(perm, NewPermission::Read) && !ty_is_freeze {
                new_prov = Provenance::Concrete { alloc_id, tag: parent_tag };
                println!(
                    "!Freeze shared reborrow: allocation {:?} inherits parent_tag {:?} (no state change)",
                    alloc_id, parent_tag
                );
            } else {
            match borrower_state.perms.permission {
                // We validate once against the pre-transition state for this reborrow.
                // If `check_borrower_tag` later starts mutating permissions, we should
                // re-evaluate whether the post-check state needs to drive these transitions.
                BorrowerPermission::Write => {
                    match perm {
                        NewPermission::Read => {
                            borrower_state.shared_borrower = Some((new_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Read;
                            println!(
                                "Updated allocation {:?} to shared borrower to {:?}",
                                alloc_id, new_tag
                            );
                        }
                        NewPermission::Write => {
                            borrower_state
                                .apply_reborrow_to_stack(parent_tag, new_tag, source, alloc_id)
                                .map_err(|msg| err_ub_format!("{msg}"))?;
                        }
                        NewPermission::TwoPhase => {
                            // Same stack logic as Write, but also record the previous effective
                            // borrower as the shared_borrower and switch to Reserved/ReservedIM.
                            let old_tag = borrower_state
                                .apply_reborrow_to_stack(parent_tag, new_tag, source, alloc_id)
                                .map_err(|msg| err_ub_format!("{msg}"))?;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission =
                                if ty_is_freeze { BorrowerPermission::Reserved }
                                else { BorrowerPermission::ReservedIM };
                            println!(
                                "Updated allocation {:?} to two-phase borrow ({}) with tag {:?}. (The stack looks like {:?})",
                                alloc_id,
                                if ty_is_freeze { "Reserved" } else { "ReservedIM" },
                                new_tag, borrower_state.exposed_stack
                            );
                        }
                    }
                }
                BorrowerPermission::Read =>
                    match perm {
                        NewPermission::Read => {
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };
                            borrower_state.shared_borrower.as_mut().unwrap().1 += 1;

                            println!(
                                "Keep allocation {:?} to shared_borrower to {:?}",
                                alloc_id, shr_tag
                            );
                        }
                        _ => {
                            throw_ub_format!(
                                "Attempting to create a non-shared reference using a shared tag {:?}",
                                borrower_state.shared_borrower
                            );
                        }
                    },
                BorrowerPermission::Frozen =>
                    match perm {
                        NewPermission::Read => {
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };
                            println!("Keep frozen allocation {:?}", alloc_id);
                        }
                        NewPermission::Write => {
                            // Frozen→Write: the new exclusive write is still derived (transitively)
                            // from the same ancestry as the previous current_borrower. Push the old
                            // tag into reborrow_chain so `release_protector` can verify ancestry.
                            let old_cb = borrower_state.current_borrower;
                            borrower_state.reborrow_chain.push(old_cb);
                            borrower_state.current_borrower = new_tag;
                            borrower_state.perms.permission = BorrowerPermission::Write;
                            println!(
                                "Updated allocation {:?} current_borrower to {:?} (reborrow_chain: {:?})",
                                alloc_id, new_tag, borrower_state.reborrow_chain
                            );
                        }
                        NewPermission::TwoPhase => {
                            let old_tag = borrower_state.current_borrower;
                            borrower_state.current_borrower = new_tag;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission =
                                if ty_is_freeze { BorrowerPermission::Reserved }
                                else { BorrowerPermission::ReservedIM };
                            println!(
                                "Updated allocation {:?} to two-phase borrow ({}) with tag {:?}",
                                alloc_id,
                                if ty_is_freeze { "Reserved" } else { "ReservedIM" },
                                new_tag
                            );
                        }
                    },
                BorrowerPermission::Reserved =>
                    match perm {
                        NewPermission::Read => {
                            // The allocation is Reserved (two-phase): the shared_borrower tag
                            // was set when the two-phase borrow was created and represents the
                            // "read-only" view that concurrent shared borrows must share.
                            // Inherit it instead of minting a new tag.
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };
                            println!(
                                "Reserved allocation {:?}: Read reborrow inherits shared tag {:?}",
                                alloc_id, shr_tag
                            );
                        }
                        NewPermission::Write => {
                            // Reserved→Write: the Write reborrow still descends from the same
                            // ancestry as the Reserved tag. Push the old current_borrower so
                            // `release_protector` can verify that protected ancestors are intact.
                            let old_cb = borrower_state.current_borrower;
                            borrower_state.reborrow_chain.push(old_cb);
                            borrower_state.current_borrower = new_tag;
                            borrower_state.perms.permission = BorrowerPermission::Write;
                            println!(
                                "Updated allocation {:?} current_borrower to {:?} (reborrow_chain: {:?})",
                                alloc_id, new_tag, borrower_state.reborrow_chain
                            );
                        }
                        NewPermission::TwoPhase => {
                            let old_tag = borrower_state.current_borrower;
                            borrower_state.current_borrower = new_tag;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission =
                                if ty_is_freeze { BorrowerPermission::Reserved }
                                else { BorrowerPermission::ReservedIM };
                            println!(
                                "Updated allocation {:?} to two-phase borrow ({}) with tag {:?}",
                                alloc_id,
                                if ty_is_freeze { "Reserved" } else { "ReservedIM" },
                                new_tag
                            );
                        }
                    },
                BorrowerPermission::ReservedIM =>
                    match perm {
                        NewPermission::Read => {
                            // Inherit the shared_borrower tag (same as Reserved::Read).
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };
                            println!(
                                "ReservedIM allocation {:?}: Read reborrow inherits shared tag {:?}",
                                alloc_id, shr_tag
                            );
                        }
                        NewPermission::Write => {
                            // Activation: identical to Reserved→Write.
                            let old_cb = borrower_state.current_borrower;
                            borrower_state.reborrow_chain.push(old_cb);
                            borrower_state.current_borrower = new_tag;
                            borrower_state.perms.permission = BorrowerPermission::Write;
                            println!(
                                "ReservedIM allocation {:?}: Write activation, new current_borrower {:?}",
                                alloc_id, new_tag
                            );
                        }
                        NewPermission::TwoPhase => {
                            let old_tag = borrower_state.current_borrower;
                            borrower_state.current_borrower = new_tag;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission =
                                if ty_is_freeze { BorrowerPermission::Reserved }
                                else { BorrowerPermission::ReservedIM };
                            println!(
                                "ReservedIM allocation {:?}: nested two-phase borrow with tag {:?}",
                                alloc_id, new_tag
                            );
                        }
                    },
            }
            } // end else (Freeze path)
        }
        interp_ok(Some(new_prov))
    }

    /// Mint a fresh tag for `place`, run the reborrow, and return a new `MPlaceTy` with
    /// updated provenance.
    ///
    /// Status: working.
    ///
    /// Interacts with: `hb_reborrow` (delegate), `hb_retag_reference` (caller).
    fn hb_retag_place(
        &mut self,
        place: &MPlaceTy<'tcx>,
        perm: NewPermission,
        source: RetagReferenceSource,
    ) -> InterpResult<'tcx, MPlaceTy<'tcx>> {
        let this = self.eval_context_mut();
        let new_tag = this.machine.borrow_tracker.as_mut().unwrap().get_mut().new_ptr();
        let new_prov = this.hb_reborrow(place, perm, new_tag, source)?;
        interp_ok(place.clone().map_provenance(|_| new_prov.unwrap()))
    }

    /// Retag a reference value: convert it to a place, retag in place, and rewrap as `ImmTy`.
    ///
    /// When `protector` is `Some`, also registers the freshly-minted tag as protected in both
    /// the global map and the current frame's list. Registration happens here (not inside
    /// `hb_retag_place`) to avoid provenance type-inference issues: `alloc_id` is captured
    /// from `place.ptr()` before retagging (alloc_id is stable), and the new tag is read back
    /// from `BorrowerState::current_borrower` after retagging.
    ///
    /// Status: working.
    ///
    /// Interacts with: `hb_retag_place` (delegate), `hb_retag_ptr_value`,
    /// `hb_retag_place_contents`.
    fn hb_retag_reference(
        &mut self,
        val: &ImmTy<'tcx>,
        perm: NewPermission,
        source: RetagReferenceSource,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        let place = this.ref_to_mplace(val)?;
        let new_place = this.hb_retag_place(&place, perm, source)?;
        interp_ok(ImmTy::from_immediate(new_place.to_ref(this), val.layout))
    }

    /// Extract the `(AllocId, BorTag)` for the allocation currently pointed to by a reference
    /// `ImmTy`. The tag returned is `current_borrower` — i.e. the freshly-minted tag after
    /// `hb_retag_reference` has run.
    ///
    /// Note: `ProtectorKind` is intentionally absent from this signature. Adding it would
    /// trigger a Rust type-inference issue in the local `RetagVisitor` struct context that
    /// resolves `ImmTy<'tcx>` as `CtfeProvenance` instead of `Provenance`.
    fn hb_get_ref_alloc_and_tag(
        &mut self,
        val: &ImmTy<'tcx>,
    ) -> InterpResult<'tcx, Option<(AllocId, BorTag)>> {
        let this = self.eval_context_mut();
        let mplace = this.ref_to_mplace(val)?;
        if let Ok((alloc_id, _, _)) = this.ptr_try_get_alloc_id(mplace.ptr(), 0) {
            if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                let bs = this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow();
                // Use the *effective* current borrower: when a raw-pointer stack is present
                // (e.g. the arg was derived via `&mut *raw_ptr`), the freshly-minted tag
                // lives in `exposed_stack.last().current_borrower`, not in `current_borrower`.
                let tag = bs
                    .exposed_stack
                    .as_deref()
                    .and_then(<[_]>::last)
                    .map(|e| e.current_borrower)
                    .unwrap_or(bs.current_borrower);
                return interp_ok(Some((alloc_id, tag)));
            }
        }
        interp_ok(None)
    }

    /// Record `(alloc_id, tag)` as a protected tag in both the global map and the current
    /// frame's list. `is_strong` selects `StrongProtector` vs `WeakProtector`.
    ///
    /// `ProtectorKind` is intentionally absent from this signature for the same reason as
    /// `hb_get_ref_alloc_and_tag` — to avoid the CtfeProvenance type-inference issue when
    /// this is called from the local `RetagVisitor` struct.
    fn hb_register_protector(
        &mut self,
        alloc_id: AllocId,
        tag: BorTag,
        is_strong: bool,
    ) -> InterpResult<'tcx> {
        let kind =
            if is_strong { ProtectorKind::StrongProtector } else { ProtectorKind::WeakProtector };
        println!("[HB]   registering protector {:?} for tag {:?} in alloc {:?}", kind, tag, alloc_id);
        let this = self.eval_context_mut();
        this.machine
            .borrow_tracker
            .as_ref()
            .unwrap()
            .borrow_mut()
            .protected_tags
            .insert(tag, kind);
        this.frame_mut()
            .extra
            .borrow_tracker
            .as_mut()
            .unwrap()
            .protected_tags
            .push((alloc_id, tag));
        interp_ok(())
    }
}

impl<'tcx> EvalContextExt<'tcx> for crate::MiriInterpCx<'tcx> {}
/// Public Hybrid Borrows surface dispatched from `borrow_tracker/mod.rs`.
///
/// Each method here is the HB-side of a generic borrow-tracker hook. The mapping is:
/// - `retag_ptr_value` → `hb_retag_ptr_value`
/// - `retag_place_contents` → `hb_retag_place_contents`
/// - `protect_place` → `hb_protect_place`
/// - `expose_tag` → `hb_expose_tag`
/// - `give_pointer_debug_name` → `hb_give_pointer_debug_name`
/// - `print_borrow_state` → `hb_print_borrow_state`
/// - `before_statement` → `hb_before_statement`
/// - `handle_polonius_anchor` → `hb_handle_polonius_anchor`
/// - `before_terminator` → `hb_before_terminator`
/// - `after_statement` → `hb_after_statement`
pub trait EvalContextExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    /// Retag a pointer value materialized by an `Rvalue::Ref` (creating a new reference).
    ///
    /// Inspects the current MIR statement to detect when the source of the reborrow is a raw
    /// pointer (so `RetagReferenceSource::RawPtr` can be threaded through). Picks
    /// `NewPermission` from the reference type plus `BorrowKind::Mut { kind: TwoPhaseBorrow }`
    /// to recognize two-phase borrows. Delegates to `hb_retag_reference`.
    ///
    /// Status: working for normal reference reborrows and two-phase recognition. Note: the
    /// `kind: RetagKind` argument is **ignored** (`let _ = kind;`) — `RetagKind::FnEntry` and
    /// `RetagKind::Default` produce identical behavior here. The protector side-effect that
    /// should differ between FnEntry and Default lives in `hb_retag_place_contents`, not
    /// here, but neither path actually installs protectors today. See [docs/open-work.md
    /// § Protectors].
    ///
    /// Interacts with: `hb_retag_reference` (delegate), `hb_retag_place_contents` (sibling
    /// dispatch path).
    fn hb_retag_ptr_value(
        &mut self,
        kind: RetagKind,
        borrow_kind: Option<rustc_middle::mir::BorrowKind>,
        val: &ImmTy<'tcx>,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        let _ = kind;
        let _ = borrow_kind;

        let mut retag_source = RetagReferenceSource::Ref;

        let frame = this.frame();
        let body = this.body();
        if let Either::Left(loc) = frame.current_loc() {
            if let Some(stmt) = body.basic_blocks[loc.block].statements.get(loc.statement_index) {
                // Look for an assignment from a reference: e.g. `_7 = &mut (*_5)`
                if let rustc_middle::mir::StatementKind::Assign(assign_data) = &stmt.kind {
                    let (_, rvalue) = &**assign_data;
                    if let rustc_middle::mir::Rvalue::Ref(_, _, borrowed_place) = rvalue {
                        let base_local = borrowed_place.local;
                        let base_ty = body.local_decls[base_local].ty;

                        match base_ty.kind() {
                            // ty::Ref(_, _pointee, mutability) => {
                            //     retag_source = RetagReferenceSource::Ref;
                            //     // println!(
                            //     //     "Retagging a value derived from reference {:?}: {:?} with mutability {:?}",
                            //     //     base_local, base_ty, mutability
                            //     // );
                            // }
                            ty::RawPtr(_mut_ty, _mutability) => {
                                retag_source = RetagReferenceSource::RawPtr;
                                println!(
                                    "Retagging a value derived from raw pointer {:?}: {:?}",
                                    base_local, base_ty
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Only retag actual references, not raw pointers
        match val.layout.ty.kind() {
            // here, it is talking about the mutability of the reference, like the y in &*y.
            ty::Ref(_, _pointee, mutability) => {
                // It's a reference, perform retagging
                // Create a fresh tag for this reborrow
                // we need to understand it for shared borrows
                // This is the RValue. And we are creating the value for LHS

                // So when the RHS is a shared borrow, LHS also gets the very same tag.
                let mut new_perm = match mutability {
                    ty::Mutability::Mut => NewPermission::Write,
                    ty::Mutability::Not => NewPermission::Read,
                };

                if borrow_kind
                    == Some(rustc_middle::mir::BorrowKind::Mut {
                        kind: rustc_middle::mir::MutBorrowKind::TwoPhaseBorrow,
                    })
                {
                    // For two-phase borrows, we start with Reserved permission
                    new_perm = NewPermission::TwoPhase; // Start as shared borrow
                    //println!("Creating two-phase borrow with Reserved permission for value: {:?}", val);
                }
                this.hb_retag_reference(val, new_perm, retag_source)
            }
            _ => {
                // Raw pointer or other type, don't retag
                interp_ok(val.clone())
            }
        }
    }

    /// Retag every pointer field reachable inside `place`. The dispatched entry for the
    /// `Retag(place)` MIR statement (and for FnEntry argument retags).
    ///
    /// A `ValueVisitor` walks aggregates and recurses into fields, retagging each pointer
    /// in place via `retag_ptr_inplace` → `hb_retag_reference`. `RetagKind::Raw` gates whether
    /// raw-pointer fields are retagged at all; `&mut`/`&` fields are always retagged when
    /// reached.
    ///
    /// Status: **partial — FnEntry is not really honored.** The visitor branches on the
    /// `kind` only to gate raw-pointer retagging and to print a `[PROTECTED]` debug suffix
    /// for `RetagKind::FnEntry`; no protector is installed and no per-frame bookkeeping is
    /// recorded. Functionally `RetagKind::FnEntry` and `RetagKind::Default` are identical
    /// here. Phase 1 of the protector plan ([docs/open-work.md § Protectors]) replaces the
    /// `[PROTECTED]` suffix with the actual side-effect.
    ///
    /// Interacts with: `hb_retag_reference` (per-pointer delegate), `protect_place` (the
    /// `protect_in_place_function_argument` machinery in `machine.rs:1789` that runs *before*
    /// this for in-place arguments).
    fn hb_retag_place_contents(
        &mut self,
        kind: RetagKind,
        place: &PlaceTy<'tcx>,
    ) -> InterpResult<'tcx> {
        println!(
            "[HB] retag_place_contents: kind={:?}, place_ty={:?}",
            kind,
            place.layout.ty
        );
        
        if kind == RetagKind::FnEntry {
            println!(
                "[HB]   -> FnEntry retag: in SB/TB, references retagged here would receive \
                 protectors (strong for &mut, weak for Box). Protectors are 'returned' \
                 (released) when the function frame ends, via release_protector on each \
                 allocation the protected tag touches."
            );
        }

        struct RetagVisitor<'ecx, 'tcx> {
            ecx: &'ecx mut MiriInterpCx<'tcx>,
            kind: RetagKind,
            in_field: bool,
        }

        impl<'ecx, 'tcx> RetagVisitor<'ecx, 'tcx> {
            #[inline(always)]
            fn retag_ptr_inplace(
                &mut self,
                place: &PlaceTy<'tcx>,
                perm: NewPermission,
                source: RetagReferenceSource,
            ) -> InterpResult<'tcx> {
                // `Retag(place)` operates on pointers already stored inside `place`.
                // So we read the old pointer value from memory, retag that pointer,
                // and then write the fresh pointer back into the same location.
                let is_fn_entry = self.kind == RetagKind::FnEntry;
                println!(
                    "[HB]   retag_ptr_inplace: ty={:?}, perm={:?}, source={:?}{}",
                    place.layout.ty,
                    perm,
                    source,
                    if is_fn_entry { " [PROTECTED]" } else { "" }
                );
                let val = self.ecx.read_immediate(&self.ecx.place_to_op(place)?)?;
                let val = self.ecx.hb_retag_reference(&val, perm, source)?;
                self.ecx.write_immediate(*val, place)?;

                // Protector registration is split into two helper methods that have no
                // ProtectorKind in their signatures — adding ProtectorKind to any method
                // called from this local struct triggers a Rust type-inference issue that
                // resolves ImmTy<'tcx> as CtfeProvenance instead of Provenance.
                if is_fn_entry {
                    // !Freeze shared refs (&T where T: !Freeze) don't get protectors.
                    // They behave like raw pointers and make no exclusivity promise, so
                    // blocking foreign writes on them would be overly strict.
                    let skip_protector = match place.layout.ty.kind() {
                        ty::Ref(_, inner_ty, ty::Mutability::Not) =>
                            !inner_ty.is_freeze(*self.ecx.tcx, self.ecx.typing_env()),
                        _ => false,
                    };
                    if !skip_protector {
                        if let Some(kind) = hb_protector_for(place.layout.ty) {
                            if let Some((alloc_id, new_tag)) =
                                self.ecx.hb_get_ref_alloc_and_tag(&val)?
                            {
                                let is_strong = matches!(kind, ProtectorKind::StrongProtector);
                                self.ecx.hb_register_protector(alloc_id, new_tag, is_strong)?;
                            }
                        }
                    }
                }
                interp_ok(())
            }
        }

        impl<'ecx, 'tcx> ValueVisitor<'tcx, MiriMachine<'tcx>> for RetagVisitor<'ecx, 'tcx> {
            type V = PlaceTy<'tcx>;

            #[inline(always)]
            fn ecx(&self) -> &MiriInterpCx<'tcx> {
                self.ecx
            }

            fn visit_box(&mut self, box_ty: ty::Ty<'tcx>, place: &PlaceTy<'tcx>) -> InterpResult<'tcx> {
                // Mirror Stacked Borrows here: only boxes using the global allocator get
                // special treatment, and the actual retag happens on the pointer field.
                if box_ty.is_box_global(*self.ecx.tcx) {
                    self.retag_ptr_inplace(place, NewPermission::Write, RetagReferenceSource::Ref)?;
                }
                interp_ok(())
            }

            fn visit_value(&mut self, place: &PlaceTy<'tcx>) -> InterpResult<'tcx> {
                // Values smaller than a pointer cannot contain any pointer we need to retag.
                // This also keeps the recursive walk cheap for ZST-heavy layouts.
                if place.layout.is_sized() && place.layout.size < self.ecx.pointer_size() {
                    return interp_ok(());
                }

                match place.layout.ty.kind() {
                    ty::Ref(_, _, mutability) => {
                        let perm = match mutability {
                            ty::Mutability::Mut => NewPermission::Write,
                            ty::Mutability::Not => NewPermission::Read,
                        };
                        self.retag_ptr_inplace(place, perm, RetagReferenceSource::Ref)?;
                    }
                    ty::RawPtr(_, mutability) => {
                        // Like SB, raw pointers are only retagged for `RetagKind::Raw`.
                        if self.kind == RetagKind::Raw {
                            let perm = match mutability {
                                ty::Mutability::Mut => NewPermission::Write,
                                ty::Mutability::Not => NewPermission::Read,
                            };
                            self.retag_ptr_inplace(place, perm, RetagReferenceSource::RawPtr)?;
                        }
                    }
                    ty::Adt(adt, _) if adt.is_box() => {
                        // Boxes need special handling via `visit_box`, so recurse into them
                        // instead of treating them like an ordinary aggregate.
                        self.walk_value(place)?;
                    }
                    _ => {
                        // For aggregates, recursively retag any pointer-valued fields they
                        // contain. We keep a small bit of state so debug prints can tell whether
                        // a retag happened in a nested field.
                        let in_field = std::mem::replace(&mut self.in_field, true);
                        self.walk_value(place)?;
                        self.in_field = in_field;
                    }
                }

                interp_ok(())
            }
        }

        let this = self.eval_context_mut();
        let mut visitor = RetagVisitor { ecx: this, kind, in_field: false };
        visitor.visit_value(place)
    }

    /// Hook called by `protect_in_place_function_argument` for each in-place function argument
    /// (`&mut`, `&` non-`UnsafeCell`, `Box`).
    ///
    /// In SB/TB this returns a freshly-protected place: the place's tag is recorded in both
    /// `FrameState.protected_tags` and `GlobalStateInner.protected_tags`, so any subsequent
    /// access through a different (or invalidated) tag while the frame is live triggers UB.
    ///
    /// Status: **not implemented** — currently clones the input place unchanged and logs.
    /// Without this, `fail/test4.rs` and `fail/test5.rs` (the protector tests) silently
    /// succeed under HB. Phase 1 of [docs/open-work.md § Protectors] is "make this function
    /// actually do something".
    ///
    /// Interacts with: `release_protector` (paired frame-exit hook), `hb_retag_place_contents`
    /// (the FnEntry retag site that should also register protectors).
    fn hb_protect_place(&mut self, place: &MPlaceTy<'tcx>) -> InterpResult<'tcx, MPlaceTy<'tcx>> {
        // Called for in-place function arguments (e.g. `fn foo(x: &mut T)`).
        // In SB/TB a protector tag is registered here so that any access through a
        // *different* pointer while the frame is live triggers UB.
        // HybridBorrows does not yet implement protectors, so this is a no-op.
        println!(
            "[HB] protect_place: ty={:?} (no-op — protectors not yet implemented in HybridBorrows; \
             in SB/TB this would register the tag as protected until frame exit)",
            place.layout.ty
        );
        interp_ok(place.clone())
    }

    /// Hook called when a tag is "exposed" (e.g. cast to an integer and back, or otherwise
    /// laundered into a wildcard provenance).
    ///
    /// Status: **not implemented** — empty body. SB and TB use this to mark the tag as
    /// available to wildcard-provenance accesses; HB ignores it. Combined with the
    /// non-concrete branch of `access` being a TODO, this means HB currently has no model of
    /// exposed pointers at all.
    fn hb_expose_tag(&self, _alloc_id: AllocId, _tag: BorTag) -> InterpResult<'tcx> {
        interp_ok(())
    }

    /// Hook for the `miri_pointer_name` intrinsic — attaches a debug name to a tag for
    /// diagnostic purposes.
    ///
    /// Status: **not implemented** — empty body. TB uses the name in tree diagnostics; HB
    /// has no equivalent surface yet, so the intrinsic is silently a no-op.
    fn hb_give_pointer_debug_name(
        &mut self,
        _ptr: Pointer,
        _nth_parent: u8,
        _name: &str,
    ) -> InterpResult<'tcx> {
        interp_ok(())
    }

    /// Hook for the `miri_print_borrow_state` intrinsic — prints the borrow tracker's view of
    /// an allocation.
    ///
    /// Status: **not implemented** — empty body. SB prints its stack; TB prints its tree;
    /// HB prints nothing. Useful to implement for debugging the access path against the
    /// `BorrowerState` field-by-field.
    fn hb_print_borrow_state(
        &mut self,
        _alloc_id: AllocId,
        _show_unnamed: bool,
    ) -> InterpResult<'tcx> {
        interp_ok(())
    }

    /// Hook called before every statement Miri executes. The Polonius-driven side of the
    /// runtime.
    ///
    /// Two responsibilities:
    /// 1. **Block-entry edge handling** — when the current location is `(block, statement_index = 0)`
    ///    and a previous block is known, look up `PoloniusFacts.predecessor_borrowers[block][pred_block]`
    ///    and apply it via `hb_apply_return_borrowers`. This implements "loans returning across
    ///    a CFG edge".
    /// 2. **Logging** — report any retags scheduled `BeforeInstruction` at the current location.
    ///
    /// Status: working for predecessor-borrower handling; the retag-logging branch is purely
    /// informational. The correctness ceiling is set by the upstream `ReturnBorrowers` shape
    /// (see `hb_restore_return_borrower_from_local`).
    ///
    /// Interacts with: `hb_apply_return_borrowers` (delegate), `PoloniusFacts.predecessor_borrowers`
    /// and `PoloniusFacts.retags` (data sources).
    fn hb_before_statement(&mut self) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        let (loc, def_id, pred_block) = {
            let frame = this.frame();
            let loc = frame.current_loc();
            let def_id = frame.instance().def_id();
            let pred_block = frame.current_pred_block();
            (loc, def_id, pred_block)
        };

        let mut selected_return_borrowers = None;
        if let Some(facts_map) = &this.machine.polonius_facts {
            if let Some(facts) = facts_map.get(&def_id) {
                if let Either::Left(target_loc) = loc {
                    if target_loc.statement_index == 0 {
                        if let Some(pred_block) = pred_block {
                            selected_return_borrowers = facts
                                .predecessor_borrowers
                                .get(&target_loc.block)
                                .and_then(|preds| preds.get(&pred_block))
                                .cloned();
                            if let Some(return_borrowers) = &selected_return_borrowers {
                                println!(
                                    "Selected predecessor path {:?} -> {:?}: {:?}",
                                    pred_block, target_loc.block, return_borrowers,
                                );
                            }
                        }
                    }
                    if let Some(retags) = facts.retags.get(&target_loc) {
                        let before_retgs: Vec<_> = retags
                            .iter()
                            .filter(|retag| {
                                retag.timing == crate::RecordedRetagTiming::BeforeInstruction
                            })
                            .collect();
                        if !before_retgs.is_empty() {
                            println!("Retags before {:?}: {:?}", target_loc, before_retgs);
                        }
                    }
                }
            }
        }

        if let Either::Left(target_loc) = loc {
            if let Some(return_borrowers) = selected_return_borrowers {
                this.hb_apply_return_borrowers(target_loc, return_borrowers)?;
            }
        }

        interp_ok(())
    }

    /// Hook called when Miri encounters a `PoloniusAnchor` MIR statement (inserted by
    /// `polonius_pass.rs`).
    ///
    /// Dispatches by `PoloniusAnchorKind`:
    /// - `MutReturnBorrower { locals }` → restore the parent's mutable borrower for each local.
    /// - `SharedReturnVar { locals }` → decrement shared refcounts for each local.
    /// - `TwoPhaseReturnBorrower { locals }` → restore the parent's borrower (same path as
    ///   `MutReturnBorrower`).
    /// - `ReturnRefArgs { locals }` → restore the parent's borrower (same path as
    ///   `MutReturnBorrower`).
    ///
    /// Status: dispatch is in place but three of the four variants share a single helper.
    /// Combined with the upstream `hb_restore_return_borrower_from_local` correctness issue,
    /// some of these calls become silent no-ops in practice. Phase 4 of the protector plan
    /// would extend this enum with `ProtectorEnd` for early protector release; see
    /// [docs/open-work.md § Protectors, Phase 4].
    ///
    /// Interacts with: `hb_restore_return_borrower_from_local`,
    /// `hb_handle_dropped_shared_return_var_from_local` (delegates), `polonius_pass.rs`
    /// (anchor inserter).
    fn hb_handle_polonius_anchor(
        &mut self,
        _id: PoloniusAnchorId,
        data: &PoloniusAnchorData,
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        match &data.kind {
            PoloniusAnchorKind::MutReturnBorrower { locals } =>
                for &local in locals {
                    println!("Polonius anchor for mutable return borrower local {:?}", local);
                    this.hb_restore_return_borrower_from_local(local, false)?;
                },
            PoloniusAnchorKind::SharedReturnVar { locals } =>
                for &local in locals {
                    println!("Polonius anchor for dropped shared return var local {:?}", local);
                    this.hb_handle_dropped_shared_return_var_from_local(local)?;
                },
            PoloniusAnchorKind::TwoPhaseReturnBorrower { locals } =>
                for &local in locals {
                    println!("Polonius anchor for two-phase return borrower local {:?}", local);
                    this.hb_restore_return_borrower_from_local(local, false)?;
                },
            PoloniusAnchorKind::ReturnRefArgs { locals } =>
                for &local in locals {
                    println!("Polonius anchor for return ref arg local {:?}", local);
                    this.hb_restore_return_borrower_from_local(local, false)?;
                },
        }

        interp_ok(())
    }

    /// Hook called before every terminator. Has two responsibilities, both call-site-related.
    ///
    /// 1. **Two-phase activation at `Call`** — for each `&mut` operand of the call, read its
    ///    pointer, look up the underlying allocation, and if it's in
    ///    `BorrowerPermission::Reserved` promote it to `Write` (clearing `shared_borrower`).
    ///    This is the activation point of two-phase borrows; the retag visitor only marks
    ///    them Reserved, the activation must happen *here*, before the callee's frame is
    ///    pushed.
    /// 2. **Return-time logging** — at `Return` terminators, look up
    ///    `PoloniusFacts.return_borrowers[loc].return_ref_args` and log it. The inline
    ///    comment ("Enforcing it here is very likely wrong") flags that this is currently
    ///    informational only; the actual restoration happens via `PoloniusAnchor` statements,
    ///    not at the terminator.
    ///
    /// Status: two-phase activation is working and exercised by `pass/test5.rs`. The
    /// `Return`-terminator branch is logging-only.
    ///
    /// Interacts with: `BorrowerPermission::{Reserved, Write}`, `hb_handle_polonius_anchor`
    /// (the actually-enforcing partner for return-time loan handling).
    fn hb_before_terminator(&mut self) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        // Phase 1: collect everything we need from immutable borrows.
        // Keeping this in a scoped block ensures all borrows are dropped before
        // phase 2, where we need to call mutable methods on `this`.
        //
        // If the terminator is a `Call`, we also collect the subset of arguments
        // whose static type is `&mut T` — those are the only candidates for
        // two-phase borrow activation.
        let mut_ref_call_args: Option<Vec<rustc_middle::mir::Operand<'tcx>>> = {
            let frame = this.frame();
            let loc = frame.current_loc();

            if let Either::Left(target_loc) = loc {
                let body = &this.body();
                if let Some(block) = body.basic_blocks.get(target_loc.block) {
                    if let Some(terminator) = &block.terminator {
                        match &terminator.kind {
                            rustc_middle::mir::TerminatorKind::Call { func, args, .. } => {
                                println!(
                                    "Call terminator at {:?}: func={:?}",
                                    target_loc, func,
                                );
                                // Only keep operands whose static type is `&mut _`.
                                let mut_refs = args
                                    .iter()
                                    .filter_map(|s| {
                                        let place = match &s.node {
                                            rustc_middle::mir::Operand::Copy(p)
                                            | rustc_middle::mir::Operand::Move(p) => p,
                                            rustc_middle::mir::Operand::Constant(_)
                                            | rustc_middle::mir::Operand::RuntimeChecks(_) =>
                                                return None,
                                        };
                                        let ty = body.local_decls[place.local].ty;
                                        matches!(
                                            ty.kind(),
                                            ty::Ref(_, _, ty::Mutability::Mut)
                                        )
                                        .then(|| s.node.clone())
                                    })
                                    .collect::<Vec<_>>();
                                Some(mut_refs)
                            }
                            rustc_middle::mir::TerminatorKind::TailCall { func, args, .. } => {
                                println!(
                                    "TailCall terminator at {:?}: func={:?}, args={:?}",
                                    target_loc, func, args,
                                );
                                None
                            }
                            rustc_middle::mir::TerminatorKind::Return => {
                                println!(
                                    "Return terminator at {:?}: returning from {} into {:?}",
                                    target_loc,
                                    frame.instance(),
                                    frame.return_place,
                                );

                                // Enforcing it here is very likely wrong.
                                if let Some(facts_map) = &this.machine.polonius_facts {
                                    if let Some(facts) =
                                        facts_map.get(&frame.instance().def_id())
                                    {
                                        if let Some(return_borrowers) =
                                            facts.return_borrowers.get(&target_loc)
                                        {
                                            if let Some(return_ref_args) =
                                                &return_borrowers.return_ref_args
                                            {
                                                let return_ref_args = return_ref_args.clone();
                                                println!(
                                                    "Reference-typed function arguments at return {:?}: {:?}",
                                                    target_loc, return_ref_args,
                                                );
                                            }
                                        }
                                    }
                                }
                                None
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }; // all borrows on `this` dropped here

        // Phase 2: activate any Reserved two-phase borrows in the call's &mut arguments.
        //
        // Per the two-phase borrow scheme, a Reserved borrow must be promoted to Write
        // at the point the function call actually happens — before the callee's frame is
        // set up.  We scan every `&mut` argument, read the pointer it contains, and
        // switch the underlying allocation from Reserved → Write.
        if let Some(args) = mut_ref_call_args {
            for arg in &args {
                let op = this.eval_operand(arg, None)?;
                let imm = this.read_immediate(&op)?;
                let mplace = this.ref_to_mplace(&imm)?;
                let Ok((alloc_id, _, prov)) = this.ptr_try_get_alloc_id(mplace.ptr(), 0)
                else {
                    continue;
                };
                let ProvenanceExtra::Concrete(tag) = prov else { continue };
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    let mut state =
                        this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
                    if state.perms.permission == BorrowerPermission::Reserved
                        || state.perms.permission == BorrowerPermission::ReservedIM
                    {
                        println!(
                            "[HB] Activating two-phase borrow: alloc={:?}, tag={:?}: {:?} → Write",
                            alloc_id, tag, state.perms.permission
                        );
                        state.perms.permission = BorrowerPermission::Write;
                        state.shared_borrower = None;
                    }
                }
            }
        }

        interp_ok(())
    }

    /// Hook called after every statement Miri executes.
    ///
    /// Status: **decorative only.** Currently prints the just-executed statement (or
    /// terminator) and a blank line. The original Polonius logic that would consume
    /// `return_borrowers` here is left in the file as commented-out code. Could be removed
    /// or repurposed; the inline `// now we don't really need hb_after_statement` comment
    /// reflects this.
    ///
    /// Interacts with: nothing semantically — purely tracing.
    fn hb_after_statement(&mut self) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        // Collect information from frame without holding mutable borrow
        let (loc, _def_id) = {
            let frame = this.frame();
            let loc = frame.current_loc();
            let def_id = frame.instance().def_id();
            (loc, def_id)
        };

        //println!("Current function DefId: {:?}", def_id);
        //println!("Location: {:?}", loc);

        // Print the statement itself if we have a Location
        if let Either::Left(target_loc) = loc {
            let body = &this.body();
            if let Some(block) = body.basic_blocks.get(target_loc.block) {
                if target_loc.statement_index < block.statements.len() {
                    let stmt = &block.statements[target_loc.statement_index];
                    println!("Statement: {:?} at {:?}", stmt, target_loc);
                } else if target_loc.statement_index == block.statements.len() {
                    // This is the terminator
                    if let Some(terminator) = &block.terminator {
                        println!("Terminator: {:?}", terminator);
                    }
                }
            }
        }

        // let mut selected_return_borrowers = None;
        // if let Some(facts_map) = &this.machine.polonius_facts {
        //     if let Some(facts) = facts_map.get(&def_id) {
        //         if let Either::Left(target_loc) = loc {
        //             selected_return_borrowers = facts.return_borrowers.get(&target_loc).cloned();
        //         }
        //     } else {
        //         println!("No Polonius facts found for {:?}", def_id);
        //     }
        // }

        // if let Either::Left(target_loc) = loc {
        //     if let Some(return_borrowers) = selected_return_borrowers {
        //         this.hb_apply_return_borrowers(target_loc, return_borrowers)?;
        //     }
        // }

        // basically we print an extra line after a statement.
        println!("");

        interp_ok(())
    }
}
