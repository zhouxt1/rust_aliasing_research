use rustc_abi::Size;
use rustc_data_structures::fx::FxHashSet;
use rustc_data_structures::either::Either;
use rustc_middle::mir::RetagKind;
use rustc_middle::ty;

use crate::borrow_tracker::{AccessKind, BorTag, GlobalState, GlobalStateInner};
use crate::*;

mod borrower;

pub use self::borrower::{BorrowerState, BorrowerPermission};

// instead of allocState, we use borrowerState

pub enum NewPermission {
    Read,
    Write,
}

pub type AllocState = BorrowerState;

/// Core per-location operations: access, dealloc, reborrow.
impl<'tcx> BorrowerState{
    /// Check if the tag matches current or previous borrower
    fn check_unique_borrower_tag(&mut self, bor_tag: BorTag) {
        if self.current_borrower == bor_tag {
            // Allow access
            //interp_ok(())
            // println!(
            //     "   Access granted: current borrower {:?} matches access tag {:?}",
            //     self.current_borrower, bor_tag
            // );
            if let Some(_prev) = self.prev_borrower {
                self.prev_borrower = None; // Clear previous borrower after successful access
            }
        } else {
            if let Some(prev_tag) = self.prev_borrower {
                // Handle case where previous borrower exists
                if prev_tag == bor_tag {

                } else {
                    println!("   Access denied: neither current {:?} nor prev borrower {:?} does not match access tag {:?}", self.current_borrower, prev_tag, bor_tag);
                }
            }
        }
    }

    fn access(
        &mut self, 
        tag: ProvenanceExtra,
        kind: AccessKind,
    ) -> InterpResult<'tcx> {


        if let ProvenanceExtra::Concrete(bor_tag) = tag {
            match self.perms.permission {
                BorrowerPermission::Read => {
                    match kind {
                        AccessKind::Read => {
                            // Allow read access
                            // this is through shared tag or borrower tag. 
                            let (shr_tag, _count) = self.shared_borrower.unwrap();
                            if shr_tag == bor_tag {
                                // Allow access
                                //interp_ok(())
                                // println!(
                                //     "   Access granted: shared borrower {:?} matches access tag {:?}",
                                //     shr_tag, bor_tag
                                // );
                            } else {
                                // I could also access through the current borrower
                                self.check_unique_borrower_tag(bor_tag);
                            }
                        }
                        AccessKind::Write => {
                            // Deny write access on a shared borrow
                            println!("   Access denied: write access on a shared borrow with tag {:?}", bor_tag);
                            return interp_ok(());
                        }
                    }

                }
                BorrowerPermission::Write => {
                    self.check_unique_borrower_tag(bor_tag);
                }
                BorrowerPermission::Frozen => {
                    // Handle frozen permission
                    match kind {
                        AccessKind::Read => {
                            // Allow read access on a frozen borrow, similar to the access 
                            return interp_ok(());
                        }
                        AccessKind::Write => {
                            // Deny write access on a frozen borrow

                            // we update the permission to Write.
                           
                            self.check_unique_borrower_tag(bor_tag);       
                            self.perms.permission = BorrowerPermission::Write;                     
                            // reset the shared_tag
                            self.shared_borrower = None;

                            return interp_ok(());
                        }
                    }
                }
            }
        } else {
            // TODO: will handle case where tag is not concrete
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

        //println!("Memory access at location: {:?}", location);
        // println!(
        //     "read access with tag {:?}: {:?}, size {}",
        //     tag,
        //     interpret::Pointer::new(alloc_id, range.start),
        //     range.size.bytes()
        // );
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
    /// Update the borrower state for an allocation with a new tag.
    /// Also tracks the previous borrower for potential conflict detection.
    fn hb_return_mut_borrower(
        &mut self,
        alloc_id: AllocId,
        new_tag: BorTag,
    ) -> InterpResult<'tcx> {
        // this return borrower should be called only when it is returning a mut borrow. Therefore, we do need to update state directly
        let this = self.eval_context_mut();
        
        if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
            let mut borrower_state = this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
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

        let ProvenanceExtra::Concrete(parent_tag) = parent_prov else {
            // If the parent provenance is not concrete, we can't track it, so we just return the original provenance
            return interp_ok(place.ptr().provenance);
        };

        match perm {
            NewPermission::Read => {
                // For a shared borrow, we want to track it differently
                let mut new_prov = Provenance::Concrete { alloc_id, tag: new_tag };
                
                // Update the borrower state if this is live data
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    let mut borrower_state = this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();

                    // If it is a brand new shared ref, then we need to update the state. 
                    match borrower_state.perms.permission  {
                        BorrowerPermission::Write => {
                            borrower_state.shared_borrower = Some((new_tag, 1));
                            borrower_state.perms.permission = BorrowerPermission::Read;
                            println!("Updated allocation {:?} to shared borrower to {:?}", alloc_id, new_tag);
                        }
                        BorrowerPermission::Read => {
                            // if it is already in Read permission, we don't update the shared_borrower, and we simply use the previous tag
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap(); // we use unwrap, since in Read state, it must have a shared tag
                            // Update new_prov to use the existing shared tag
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };

                            // we need to know if parent prov is from 'current borrower' or from 'shared borrower'
                            // if it is from a shared borrower, then simply update it
                            if parent_tag != shr_tag {
                                // if the parent prov is from shared borrower, then we don't need to increase the count. 
                                borrower_state.shared_borrower.as_mut().unwrap().1 += 1;
                            }

                            println!("Keep allocation {:?} to shared_borrower to {:?}", alloc_id, shr_tag);
                        }
                        BorrowerPermission::Frozen => {
                            // frozen is when we have exited the shared mode, however, we are not sure if it is still in use, since we can still have 
                            // shared raw pointers alive. 
                            let (shr_tag, _count) = borrower_state.shared_borrower.unwrap();
                                // If we have a shared tag, we can use it
                            new_prov = Provenance::Concrete { alloc_id, tag: shr_tag };

                            // update it back to Read state
                            borrower_state.perms.permission = BorrowerPermission::Read;
                            println!("Update frozen allocation {:?} to shared_borrower to {:?}", alloc_id, shr_tag);
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
                let new_prov = Provenance::Concrete { alloc_id, tag: new_tag };
                
                // Update the borrower state if this is live data
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut().current_borrower = new_tag;
                    println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, new_tag);
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
        val: &ImmTy<'tcx>,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        
        // Only retag actual references, not raw pointers
        match val.layout.ty.kind() {
            // here, it is talking about the mutability of the reference, like the y in &*y. 
            ty::Ref(_, _pointee, mutability) => {
                // It's a reference, perform retagging
                // Create a fresh tag for this reborrow
                // we need to understand it for shared borrows
                // This is the RValue. And we are creating the value for LHS

                // So when the RHS is a shared borrow, LHS also gets the very same tag. 
                let new_perm = match mutability {
                    ty::Mutability::Mut => NewPermission::Write,
                    ty::Mutability::Not => NewPermission::Read,
                };
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

    fn hb_print_borrow_state(&mut self, _alloc_id: AllocId, _show_unnamed: bool) -> InterpResult<'tcx> {
        interp_ok(())
    }

    fn hb_after_statement(&mut self) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        // Collect information from frame without holding mutable borrow
        let (loc, def_id) = {
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
                    println!("Statement: {:?}", stmt);
                } else if target_loc.statement_index == block.statements.len() {
                    // This is the terminator
                    if let Some(terminator) = &block.terminator {
                        println!("Terminator: {:?}", terminator);
                    }
                }
            }
        }


        // Check for return borrowers at this location
        let has_facts = this.machine.polonius_facts.is_some();
        if has_facts {
            if let Some(facts_map) = &this.machine.polonius_facts {
                if let Some(facts) = facts_map.get(&def_id) {
                    // Extract Location from Either (Left = Location, Right = Span)
                    if let Either::Left(target_loc) = loc {
                        // print loan live at the current location
                        //let loc_index = facts.location_table.to_index(target_loc);
                        if let Some(loans) = facts.loan_live_at.get(&target_loc) {
                            // this is printed for debug only. We don't need to keep track of these in production. 
                            println!("      Loan live at {:?}: {:?}", target_loc, loans);                             
                        }

                        if let Some(return_borrowers) = facts.return_borrowers.get(&target_loc) {
                            // Extract both mut and shared borrowers before processing
                            let mut_locals = return_borrowers.mut_borrows.clone();
                            let shared_locals = return_borrowers.shared.clone();
                            drop(facts_map); // Explicitly drop to release immutable borrow
                            
                            // Process mut_borrows
                            if let Some(mut_locals) = mut_locals {
                                println!("Found return mut borrowers at {:?}: {:?}", target_loc, mut_locals);
                                
                                for local in mut_locals {
                                    // Convert local to a place and get allocation information
                                    let src = this.local_to_place(local)?;
                                    
                                    // Get the pointer and extract tag for this local
                                    let (alloc_id, _, tag) = if src.layout.ty.is_ref() {
                                        // It's a reference: read it and follow the pointer
                                        let imm = this.read_immediate(&src)?;
                                        let mplace = this.ref_to_mplace(&imm)?;
                                        this.ptr_get_alloc_id(mplace.ptr(), 0)?
                                    } else {
                                        // It's an owned value: force it into allocation
                                        let mplace = this.force_allocation(&src)?;
                                        this.ptr_get_alloc_id(mplace.ptr(), 0)?
                                    };
                                    
                                    // Update the borrower state with the extracted tag
                                    if let ProvenanceExtra::Concrete(bor_tag) = tag {
                                        
                                        this.hb_return_mut_borrower(alloc_id, bor_tag)?;
                                        println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
                                    }
                                }
                            }
                            
                            // process shared borrows
                            if let Some(shared_loans) = shared_locals {
                                println!("Found return shared borrowers at {:?}: {:?}", target_loc, shared_loans);

                                // Question, how does it know if these shared borrows are from the same memory location or not? 
                                // It probably don't know. So when we likely need to add a counter. Basically, when the counter goes to 0,
                                // it means that the shared borrow is no longer active and transition to Frozen state. 
                                for local in shared_loans {
                                    let src = this.local_to_place(local)?;
                                    
                                    // Get the pointer and extract tag for this local
                                    let (alloc_id, _, tag) = if src.layout.ty.is_ref() {
                                        // It's a reference: read it and follow the pointer
                                        let imm = this.read_immediate(&src)?;
                                        let mplace = this.ref_to_mplace(&imm)?;
                                        this.ptr_get_alloc_id(mplace.ptr(), 0)?
                                    } else {
                                        // It's an owned value: force it into allocation
                                        let mplace = this.force_allocation(&src)?;
                                        this.ptr_get_alloc_id(mplace.ptr(), 0)?
                                    };
                                    
                                    if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                                        let mut borrower_state = this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut();
                                        if borrower_state.perms.permission == BorrowerPermission::Read {
                                            let shared_borrower_info = borrower_state.shared_borrower.as_mut().unwrap();
                                            shared_borrower_info.1 -= 1;
                                            println!("Updated allocation {:?} with {:?} shared refs", alloc_id, shared_borrower_info.1);
                                            if shared_borrower_info.1 == 0 {
                                                // transition to Frozen state
                                                borrower_state.perms.permission = BorrowerPermission::Frozen;
                                                println!("Shared borrower for allocation {:?} is now frozen", alloc_id);
                                            }
                                        }

                                    }

                                }
                            }
                        }
                    }
                } else {
                    println!("No Polonius facts found for {:?}", def_id);
                }
            }
        }

        // basically we print an extra line after a statement. 
        println!("");

        interp_ok(())
    }
}

