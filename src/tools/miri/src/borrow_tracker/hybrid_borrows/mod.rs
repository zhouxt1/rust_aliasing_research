use rustc_abi::Size;
use rustc_data_structures::either::Either;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::{
    Local, PoloniusAnchorData, PoloniusAnchorId, PoloniusAnchorKind, RetagKind,
};
use rustc_middle::ty;

use crate::borrow_tracker::{AccessKind, BorTag, GlobalState, GlobalStateInner};
use crate::*;

mod borrower;

pub use self::borrower::{BorrowerPermission, BorrowerState, RawPointerStack};

// instead of allocState, we use borrowerState

#[derive(Debug)]
pub enum NewPermission {
    Read,
    Write,
    TwoPhase,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RetagReferenceSource {
    Ref,
    RawPtr,
}

pub type AllocState = BorrowerState;
type BorrowCheckResult = Result<(), String>;

/// Core per-location operations: access, dealloc, reborrow.
impl<'tcx> BorrowerState {
    /// Check if the tag matches current or previous borrower
    fn check_unique_borrower_tag(&mut self, bor_tag: BorTag) -> BorrowCheckResult {
        if self.current_borrower == bor_tag {
            println!(
                "   Access granted: current borrower {:?} matches access tag {:?}",
                self.current_borrower, bor_tag
            );

            if let Some(_prev) = self.prev_borrower {
                self.prev_borrower = None; // Clear previous borrower after successful access
            }
            Ok(())
        } else {
            if let Some(prev_tag) = self.prev_borrower {
                // Handle case where previous borrower exists
                if prev_tag == bor_tag {
                    println!(
                        "   Access granted: previous borrower {:?} matches access tag {:?}",
                        prev_tag, bor_tag
                    );
                    Ok(())
                } else {
                    Err(format!(
                        "Access denied: neither current {:?} nor prev borrower {:?} matches access tag {:?}",
                        self.current_borrower, prev_tag, bor_tag
                    ))
                }
            } else {
                Err(format!(
                    "Access denied: neither current {:?} nor prev borrower exists for access tag {:?}",
                    self.current_borrower, bor_tag
                ))
            }
        }
    }

    fn check_raw_pointer_stack(&mut self, bor_tag: BorTag) -> BorrowCheckResult {
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
                    exposed_stack.truncate(idx + 1);

                    if entry.base_pointer != lower_current_borrower {
                        self.prev_borrower = Some(entry.base_pointer);
                        exposed_stack.pop();
                    }

                    if exposed_stack.is_empty() {
                        self.exposed_stack = None;
                    }

                    println!(
                        "   Raw pointer stack access {:?}: matched base pointer {:?}, lower borrower {:?}",
                        bor_tag, entry.base_pointer, lower_current_borrower
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

    fn check_borrower_tag(&mut self, bor_tag: BorTag, kind: AccessKind) -> BorrowCheckResult {
        match self.perms.permission {
            BorrowerPermission::Read => {
                match kind {
                    AccessKind::Read => {
                        // Allow read access
                        // this is through shared tag or borrower tag.
                        let (shr_tag, _count) = self.shared_borrower.unwrap();
                        if shr_tag == bor_tag {
                            Ok(())
                        } else {
                            // I could also access through the current borrower
                            if self.exposed_stack.is_some() {
                                self.check_raw_pointer_stack(bor_tag)
                            } else {
                                self.check_unique_borrower_tag(bor_tag)
                            }
                        }
                    }
                    AccessKind::Write => {
                        // Deny write access on a shared borrow
                        Err(format!(
                            "Access denied: write access on a shared borrow with tag {:?}",
                            bor_tag
                        ))
                    }
                }
            }
            BorrowerPermission::Write =>
                if self.exposed_stack.is_some() {
                    self.check_raw_pointer_stack(bor_tag)
                } else {
                    self.check_unique_borrower_tag(bor_tag)
                },
            BorrowerPermission::Frozen => {
                // Handle frozen permission
                match kind {
                    AccessKind::Read => {
                        // Allow read access on a frozen borrow, similar to the access
                        // but we need to check the tags as well.
                        let (shr_tag, _count) = self.shared_borrower.unwrap();
                        if shr_tag == bor_tag {
                            Ok(())
                        } else {
                            // I could also access through the current borrower
                            self.check_unique_borrower_tag(bor_tag)
                        }
                    }
                    AccessKind::Write => {
                        // Deny write access on a frozen borrow

                        // we update the permission to Write.

                        self.check_unique_borrower_tag(bor_tag)?;
                        self.perms.permission = BorrowerPermission::Write;
                        // reset the shared_tag
                        self.shared_borrower = None;

                        Ok(())
                    }
                }
            }
            BorrowerPermission::Reserved => {
                // Handle reserved permission (e.g., for two-phase borrows)
                match kind {
                    AccessKind::Read => {
                        // We first check if it matches the unique borrower tag.
                        if self.check_unique_borrower_tag(bor_tag).is_ok() {
                            Ok(())
                        } else {
                            // if not, check the shared borrower tag
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
                        self.check_unique_borrower_tag(bor_tag)?;
                        // This probably also needs some rewrite, since we don't need to check prev_borrower. Since it is an active Reserved State.

                        // now we need to make it Unique.
                        self.perms.permission = BorrowerPermission::Write;
                        self.shared_borrower = None;

                        Ok(())
                    }
                }
            }
        }
    }

    fn access(&mut self, tag: ProvenanceExtra, kind: AccessKind) -> InterpResult<'tcx> {
        if let ProvenanceExtra::Concrete(bor_tag) = tag {
            println!(
                "Access with concrete tag {:?}, kind {:?}, and current permission {:?}",
                bor_tag, kind, self.perms.permission
            );
            self.check_borrower_tag(bor_tag, kind).map_err(|msg| err_ub_format!("{msg}"))?;
        } else {
            // TODO: will handle case where tag is not concrete
            println!("   Access with non-concrete access tag {:?}", tag);
        }
        interp_ok(())
    }
}

impl BorrowerState {
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

    pub fn before_memory_access<'tcx>(
        &mut self,
        kind: AccessKind,
        _alloc_id: AllocId,
        tag: ProvenanceExtra,
        _range: AllocRange,
        _machine: &MiriMachine<'tcx>,
    ) -> InterpResult<'tcx> {
        //let location = machine.threads.active_thread_stack().last().map(|frame| frame.current_loc());

        self.access(tag, kind)?;

        interp_ok(())
    }

    pub fn before_memory_deallocation<'tcx>(
        &mut self,
        _alloc_id: AllocId,
        _prov_extra: ProvenanceExtra,
        _size: Size,
        _machine: &MiriMachine<'tcx>,
    ) -> InterpResult<'tcx> {
        interp_ok(())
    }

    /// Apply a mutable reborrow to the exposed-stack state, respecting both `Ref` and `RawPtr`
    /// sources.  The function encapsulates all of the "where does the new tag go in the stack?"
    /// logic that used to live inline inside `NewPermission::Write`.
    ///
    /// Returns the *previous effective borrower tag* (top-of-stack `current_borrower` when a
    /// stack exists, or `self.current_borrower` when it does not).  The caller can use this
    /// value as the `shared_borrower` entry when creating a two-phase (Reserved) borrow.
    ///
    /// # Errors
    /// Returns a string describing UB if the reborrow is not permitted.
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
                    self.current_borrower = new_tag;
                    println!(
                        "Updated allocation {:?} current_borrower to {:?}",
                        alloc_id, new_tag
                    );
                }
            }
        }

        Ok(old_tag)
    }

    pub fn remove_unreachable_tags(&self, _tags: &FxHashSet<BorTag>) {}

    pub fn release_protector<'tcx>(
        &self,
        _machine: &MiriMachine<'tcx>,
        _global: &GlobalState,
        tag: BorTag,
        alloc_id: AllocId,
    ) -> InterpResult<'tcx> {
        // Called when a stack frame ends to "return" (release) any protectors that were
        // granted at FnEntry retag time. In SB/TB this would check that the protected
        // tag's memory is still accessible and then remove the protection entry, allowing
        // future conflicting accesses to proceed. HybridBorrows does not yet track
        // protectors, so this is a no-op.
        println!(
            "[HB] release_protector: tag={:?}, alloc_id={:?} \
             (no-op — protectors not yet implemented in HybridBorrows; \
             in SB/TB this fires at frame exit to end the protected lifetime of the tag)",
            tag, alloc_id
        );
        interp_ok(())
    }
}

impl VisitProvenance for BorrowerState {
    fn visit_provenance(&self, visit: &mut VisitWith<'_>) {
        visit(None, Some(self.current_borrower));
    }
}

impl<'tcx> EvalContextPrivExt<'tcx> for crate::MiriInterpCx<'tcx> {}
trait EvalContextPrivExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    fn hb_restore_return_borrower_from_local(&mut self, local: Local) -> InterpResult<'tcx> {
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
            this.hb_return_mut_borrower(alloc_id, bor_tag)?;
            println!("Updated return allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
        }

        interp_ok(())
    }

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
                this.hb_restore_return_borrower_from_local(local)?;
            }
        }

        if let Some(two_phase_locals) = two_phase_locals {
            println!(
                "Found return two-phase borrowers at {:?}: {:?}",
                target_loc, two_phase_locals,
            );

            for local in two_phase_locals {
                this.hb_restore_return_borrower_from_local(local)?;
            }
        }

        interp_ok(())
    }

    /// Update the borrower state for an allocation with a new tag.
    /// Also tracks the previous borrower for potential conflict detection.
    fn hb_return_mut_borrower(&mut self, alloc_id: AllocId, new_tag: BorTag) -> InterpResult<'tcx> {
        // this return borrower should be called only when it is returning a mut borrow. Therefore, we do need to update state directly
        let this = self.eval_context_mut();

        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state =
                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
            let old_tag = borrower_state.current_borrower;
            borrower_state.current_borrower = new_tag;
            borrower_state.prev_borrower = Some(old_tag);

            // if it is in 'read state', we move it to frozen directly.
            if borrower_state.perms.permission == BorrowerPermission::Read {
                //borrower_state.shared_borrower = None;
                borrower_state.perms.permission = BorrowerPermission::Frozen;
            }
        }

        interp_ok(())
    }

    /// Perform the core reborrowing logic.
    /// This creates a new tag and updates the borrower state.
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

        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state =
                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();

            // we need to check the borrower state by accessing it. It might or might not change the current memory state.
            borrower_state
                .check_borrower_tag(parent_tag, AccessKind::Read)
                .map_err(|msg| {
                    err_ub_format!(
                        "invalid parent tag {:?} for reborrow at allocation {:?} with permission {:?}: {}",
                        parent_tag,
                        alloc_id,
                        borrower_state.perms.permission,
                        msg
                    )
                })?;

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
                            // borrower as the shared_borrower and switch to Reserved permission.
                            let old_tag = borrower_state
                                .apply_reborrow_to_stack(parent_tag, new_tag, source, alloc_id)
                                .map_err(|msg| err_ub_format!("{msg}"))?;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Reserved;
                            println!(
                                "Updated allocation {:?} to two-phase borrow with tag {:?}. (The stack looks like {:?})",
                                alloc_id, new_tag, borrower_state.exposed_stack
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
                            borrower_state.current_borrower = new_tag;
                            borrower_state.perms.permission = BorrowerPermission::Write;
                            println!(
                                "Updated allocation {:?} current_borrower to {:?}",
                                alloc_id, new_tag
                            );
                        }
                        NewPermission::TwoPhase => {
                            let old_tag = borrower_state.current_borrower;
                            borrower_state.current_borrower = new_tag;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Reserved;
                            println!(
                                "Updated allocation {:?} to two-phase borrow with tag {:?}",
                                alloc_id, new_tag
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
                            borrower_state.current_borrower = new_tag;
                            borrower_state.perms.permission = BorrowerPermission::Write;
                            println!(
                                "Updated allocation {:?} current_borrower to {:?}",
                                alloc_id, new_tag
                            );
                        }
                        NewPermission::TwoPhase => {
                            let old_tag = borrower_state.current_borrower;
                            borrower_state.current_borrower = new_tag;
                            borrower_state.shared_borrower = Some((old_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Reserved;
                            println!(
                                "Updated allocation {:?} to two-phase borrow with tag {:?}",
                                alloc_id, new_tag
                            );
                        }
                    },
            }
        }
        interp_ok(Some(new_prov))
    }

    /// Retag a place (e.g., when assigning a reference to a location)
    fn hb_retag_place(
        &mut self,
        place: &MPlaceTy<'tcx>,
        perm: NewPermission,
        source: RetagReferenceSource,
    ) -> InterpResult<'tcx, MPlaceTy<'tcx>> {
        let this = self.eval_context_mut();

        // Create a fresh tag for this reborrow
        let new_tag = this.machine.borrow_tracker.as_mut().unwrap().get_mut().new_ptr();

        // Perform the reborrow logic
        let new_prov = this.hb_reborrow(place, perm, new_tag, source)?;

        // Return the place with updated provenance
        interp_ok(place.clone().map_provenance(|_| new_prov.unwrap()))
    }

    /// Retag an individual reference
    fn hb_retag_reference(
        &mut self,
        val: &ImmTy<'tcx>,
        perm: NewPermission,
        source: RetagReferenceSource,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        // println!("Retagging reference {:?} with source {:?}", val.layout.ty, source);
        let place = this.ref_to_mplace(val)?;
        let new_place = this.hb_retag_place(&place, perm, source)?;
        interp_ok(ImmTy::from_immediate(new_place.to_ref(this), val.layout))
    }
}

impl<'tcx> EvalContextExt<'tcx> for crate::MiriInterpCx<'tcx> {}
pub trait EvalContextExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    /// Retag a pointer value (called when creating references)
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

    /// Retag all pointers stored in a place
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
                println!(
                    "[HB]   retag_ptr_inplace: ty={:?}, perm={:?}, source={:?}{}",
                    place.layout.ty,
                    perm,
                    source,
                    if self.kind == RetagKind::FnEntry { " [PROTECTED]" } else { "" }
                );
                let val = self.ecx.read_immediate(&self.ecx.place_to_op(place)?)?;
                let val = self.ecx.hb_retag_reference(&val, perm, source)?;
                self.ecx.write_immediate(*val, place)?;
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

    fn hb_expose_tag(&self, _alloc_id: AllocId, _tag: BorTag) -> InterpResult<'tcx> {
        interp_ok(())
    }

    fn hb_give_pointer_debug_name(
        &mut self,
        _ptr: Pointer,
        _nth_parent: u8,
        _name: &str,
    ) -> InterpResult<'tcx> {
        interp_ok(())
    }

    fn hb_print_borrow_state(
        &mut self,
        _alloc_id: AllocId,
        _show_unnamed: bool,
    ) -> InterpResult<'tcx> {
        interp_ok(())
    }

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
                    this.hb_restore_return_borrower_from_local(local)?;
                },
            PoloniusAnchorKind::SharedReturnVar { locals } =>
                for &local in locals {
                    println!("Polonius anchor for dropped shared return var local {:?}", local);
                    this.hb_handle_dropped_shared_return_var_from_local(local)?;
                },
            PoloniusAnchorKind::TwoPhaseReturnBorrower { locals } =>
                for &local in locals {
                    println!("Polonius anchor for two-phase return borrower local {:?}", local);
                    this.hb_restore_return_borrower_from_local(local)?;
                },
            PoloniusAnchorKind::ReturnRefArgs { locals } =>
                for &local in locals {
                    println!("Polonius anchor for return ref arg local {:?}", local);
                    this.hb_restore_return_borrower_from_local(local)?;
                },
        }

        interp_ok(())
    }

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
                    if state.perms.permission == BorrowerPermission::Reserved {
                        println!(
                            "[HB] Activating two-phase borrow: alloc={:?}, tag={:?}: Reserved → Write",
                            alloc_id, tag
                        );
                        state.perms.permission = BorrowerPermission::Write;
                        state.shared_borrower = None;
                    }
                }
            }
        }

        interp_ok(())
    }

    // now we don't really need hb_after_statement
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
