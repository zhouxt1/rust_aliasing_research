use rustc_abi::Size;
use rustc_data_structures::either::Either;
use rustc_data_structures::fx::FxHashSet;
use rustc_middle::mir::{RetagKind, PoloniusAnchorData, PoloniusAnchorKind, PoloniusAnchorId, Local};
use rustc_middle::ty;

use crate::borrow_tracker::{AccessKind, BorTag, GlobalState, GlobalStateInner};
use crate::*;


mod borrower;

pub use self::borrower::{BorrowerPermission, BorrowerState};


// instead of allocState, we use borrowerState

pub enum NewPermission {
    Read,
    Write,
    TwoPhase,
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
                            self.check_unique_borrower_tag(bor_tag)
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
            BorrowerPermission::Write => self.check_unique_borrower_tag(bor_tag),
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
            if let Err(msg) = self.check_borrower_tag(bor_tag, kind) {
                println!("   {}", msg);
            }
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

    pub fn remove_unreachable_tags(&self, _tags: &FxHashSet<BorTag>) {}

    pub fn release_protector<'tcx>(
        &self,
        _machine: &MiriMachine<'tcx>,
        _global: &GlobalState,
        _tag: BorTag,
        _alloc_id: AllocId,
    ) -> InterpResult<'tcx> {
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
            println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
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
    ) -> InterpResult<'tcx, Option<Provenance>> {
        let this = self.eval_context_mut();

        // we need to log creation later. We skip it for now

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

        // let's first check if the tag is valid

        match perm {
            NewPermission::Read => {
                // For a shared borrow, we want to track it differently
                let mut new_prov = Provenance::Concrete { alloc_id, tag: new_tag };

                // Update the borrower state if this is live data
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    let mut borrower_state =
                        this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();

                    if let Err(msg) =
                        borrower_state.check_borrower_tag(parent_tag, AccessKind::Read)
                    {
                        // If the tag is not valid, we can't track it.
                        // For now, we just print the information.
                        println!(
                            "Invalid parent tag {:?} for reborrow at allocation {:?} with permission {:?}: {}",
                            parent_tag, alloc_id, borrower_state.perms.permission, msg
                        );
                    }

                    // If it is a brand new shared ref, then we need to update the state.
                    match borrower_state.perms.permission {
                        BorrowerPermission::Write => {
                            borrower_state.shared_borrower = Some((new_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Read;
                            // Shouldn't we need to access the previous tag somewhere?
                            println!(
                                "Updated allocation {:?} to shared borrower to {:?}",
                                alloc_id, new_tag
                            );
                        }
                        BorrowerPermission::Read => {
                            // if it is already in Read permission, we don't update the shared_borrower, and we simply use the previous tag
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap(); // we use unwrap, since in Read state, it must have a shared tag
                            // Update new_prov to use the existing shared tag
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };

                            // increase the shared-borrower count by 1
                            borrower_state.shared_borrower.as_mut().unwrap().1 += 1;

                            println!(
                                "Keep allocation {:?} to shared_borrower to {:?}",
                                alloc_id, shr_tag
                            );
                        }
                        BorrowerPermission::Frozen => {
                            // frozen is when we have exited the shared mode, however, we are not sure if it is still in use, since we can still have
                            // shared raw pointers alive.
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                            // If we have a shared tag, we can use it
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };

                            // update it back to Read state
                            borrower_state.perms.permission = BorrowerPermission::Read;
                            println!(
                                "Update frozen allocation {:?} to shared_borrower to {:?}",
                                alloc_id, shr_tag
                            );
                        }
                        BorrowerPermission::Reserved => {
                            // Handle reserved permission.
                            // we would allow a read.
                            // in this case, we don't want to update the new permission.
                        }
                    }

                    // If it is an old shared ref

                    //let old_shr_tag = borrower_state.current_borrower;
                    //println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, new_tag);
                }

                interp_ok(Some(new_prov))
            }
            NewPermission::Write => {
                // For a mutable borrow, we create a new tag and update borrower state
                // Create new provenance with the new tag
                // we need to check the previous tag before we can update the borrower state
                let new_prov = Provenance::Concrete { alloc_id, tag: new_tag };

                // Update the borrower state if this is live data
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    let mut borrower_state =
                        this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();

                    if let Err(msg) =
                        borrower_state.check_borrower_tag(parent_tag, AccessKind::Read)
                    {
                        // If the tag is not valid, we can't track it.
                        // For now, we just print the information.
                        println!(
                            "Invalid parent tag {:?} for reborrow at allocation {:?}: {}",
                            parent_tag, alloc_id, msg
                        );
                    }

                    borrower_state.current_borrower = new_tag;
                    println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, new_tag);
                }

                interp_ok(Some(new_prov))
            }
            NewPermission::TwoPhase => {
                // For a two-phase borrow, we start with a reserved state and the new tag
                let new_prov = Provenance::Concrete { alloc_id, tag: new_tag };

                // Update the borrower state if this is live data
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    let mut borrower_state =
                        this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
                    let old_tag = borrower_state.current_borrower;

                    if let Err(msg) =
                        borrower_state.check_borrower_tag(parent_tag, AccessKind::Read)
                    {
                        // If the tag is not valid, we can't track it.
                        // For now, we just print the information.
                        println!(
                            "Invalid parent tag {:?} for reborrow at allocation {:?}: {}",
                            parent_tag, alloc_id, msg
                        );
                    }

                    borrower_state.current_borrower = new_tag;
                    borrower_state.shared_borrower = Some((old_tag, 1));
                    borrower_state.perms.permission = BorrowerPermission::Reserved;
                    println!(
                        "Updated allocation {:?} to two-phase borrow with tag {:?}",
                        alloc_id, new_tag
                    );
                }

                interp_ok(Some(new_prov))
            }
        }
    }

    /// Retag a place (e.g., when assigning a reference to a location)
    fn hb_retag_place(
        &mut self,
        place: &MPlaceTy<'tcx>,
        perm: NewPermission,
    ) -> InterpResult<'tcx, MPlaceTy<'tcx>> {
        let this = self.eval_context_mut();

        // Create a fresh tag for this reborrow
        let new_tag = this.machine.borrow_tracker.as_mut().unwrap().get_mut().new_ptr();

        // Perform the reborrow logic
        let new_prov = this.hb_reborrow(place, perm, new_tag)?;

        // Return the place with updated provenance
        interp_ok(place.clone().map_provenance(|_| new_prov.unwrap()))
    }

    /// Retag an individual reference
    fn hb_retag_reference(
        &mut self,
        val: &ImmTy<'tcx>,
        perm: NewPermission,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        let place = this.ref_to_mplace(val)?;
        let new_place = this.hb_retag_place(&place, perm)?;
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
                this.hb_retag_reference(val, new_perm)
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
        _kind: RetagKind,
        _place: &PlaceTy<'tcx>,
    ) -> InterpResult<'tcx> {
        // For hybrid borrows, we don't recursively retag place contents yet
        // This would require a visitor pattern similar to tree_borrows
        interp_ok(())
    }

    fn hb_protect_place(&mut self, place: &MPlaceTy<'tcx>) -> InterpResult<'tcx, MPlaceTy<'tcx>> {
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
            PoloniusAnchorKind::MutReturnBorrower { locals } => {
                for &local in locals {
                    println!(
                        "Polonius anchor for mutable return borrower local {:?}",
                        local
                    );
                    this.hb_restore_return_borrower_from_local(local)?;
                }
            }
            PoloniusAnchorKind::SharedReturnVar { .. }
            | PoloniusAnchorKind::TwoPhaseReturnBorrower { .. } => {}
        }

        interp_ok(())
    }

    fn hb_before_terminator(&mut self) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();
        let frame = this.frame();
        let loc = frame.current_loc();

        if let Either::Left(target_loc) = loc {
            let body = &this.body();
            if let Some(block) = body.basic_blocks.get(target_loc.block) {
                if let Some(terminator) = &block.terminator {
                    //println!("Before terminator at {:?}: {:?}", target_loc, terminator);

                    match &terminator.kind {
                        rustc_middle::mir::TerminatorKind::Call { func, .. } => {
                            println!("Call terminator at {:?}: func={:?}", target_loc, func,);
                        }
                        rustc_middle::mir::TerminatorKind::TailCall { func, args, .. } => {
                            println!(
                                "TailCall terminator at {:?}: func={:?}, args={:?}",
                                target_loc, func, args,
                            );
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
                                if let Some(facts) = facts_map.get(&frame.instance().def_id()) {
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
                        }
                        _ => {}
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
