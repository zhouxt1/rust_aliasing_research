use rustc_abi::Size;
use rustc_data_structures::fx::FxHashSet;
use rustc_data_structures::either::Either;
use rustc_middle::mir::RetagKind;

use crate::borrow_tracker::{AccessKind, BorTag, GlobalState, GlobalStateInner};
use crate::*;

// instead of allocState, we use borrowerState



#[derive(Debug, Clone)]
pub struct BorrowerState {
    current_borrower: BorTag,
    
}

pub type AllocState = BorrowerState;

impl BorrowerState {
    pub fn new_allocation(
        id: AllocId,
        _alloc_size: Size,
        state: &mut GlobalStateInner,
        _kind: MemoryKind,
        machine: &MiriMachine<'_>,
    ) -> Self {
        let tag = state.root_ptr_tag(id, machine);
        BorrowerState { current_borrower: tag }
    }

    pub fn before_memory_access<'tcx>(
        &mut self,
        _kind: AccessKind,
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

        if let ProvenanceExtra::Concrete(bor_tag) = tag {
            if self.current_borrower == bor_tag {
                // Allow access
                //interp_ok(())
                // println!(
                //     "   Access granted: current borrower {:?} matches access tag {:?}",
                //     self.current_borrower, bor_tag
                // );
            } else {
                println!(
                    "   Access denied: current borrower {:?} does not match access tag {:?}",
                    self.current_borrower, bor_tag
                );
            }
        } else {
            
        }
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

pub trait EvalContextExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    fn hb_retag_ptr_value(
        &mut self,
        kind: RetagKind,
        val: &ImmTy<'tcx>,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        this.hb_retag_reference(val, kind)
    }

    fn hb_retag_reference(
        &mut self,
        val: &ImmTy<'tcx>,
        kind: RetagKind,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        let this = self.eval_context_mut();
        let place = this.ref_to_mplace(val)?;
        let (alloc_id, _base_offset, orig_tag) = this.ptr_get_alloc_id(place.ptr(), 0)?;
        
        // For RAW retags, inherit the parent pointer's tag without creating a new one
        let final_tag = match kind {
            RetagKind::Raw => {
                // Raw borrows don't get retagged, inherit the original tag
                match orig_tag {
                    ProvenanceExtra::Concrete(tag) => tag,
                    ProvenanceExtra::Wildcard => {
                        // If it's wildcard, create a new tag
                        this.machine.borrow_tracker.as_mut().unwrap().get_mut().new_ptr()
                    }
                }
            }
            _ => {
                // For other retag kinds, create a new tag and update the borrower state
                let new_tag = this.machine.borrow_tracker.as_mut().unwrap().get_mut().new_ptr();
                if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                    this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut().current_borrower = new_tag;
                }
                println!("Retagging reference {:?} to new tag {:?}", val, new_tag);
                new_tag
            }
        };
        
        let new_place = place.map_provenance(|_| Provenance::Concrete { alloc_id, tag: final_tag });
        interp_ok(ImmTy::from_immediate(new_place.to_ref(this), val.layout))
    }

    fn hb_retag_place_contents(
        &mut self,
        _kind: RetagKind,
        _place: &PlaceTy<'tcx>,
    ) -> InterpResult<'tcx> {
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
                        if let Some(return_borrowers) = facts.return_borrowers.get(&target_loc) {
                            // Process mut_borrows
                            if let Some(mut_locals) = &return_borrowers.mut_borrows {
                                println!("Found return mut borrowers at {:?}: {:?}", target_loc, mut_locals);
                                // Collect locals to process (to avoid holding immutable borrow while mutating)
                                let locals_to_process: Vec<_> = mut_locals.iter().copied().collect();
                                drop(facts_map); // Explicitly drop to release immutable borrow
                                
                                for local in locals_to_process {
                                    // Convert local to a value and get allocation information
                                    let src = this.local_to_place(local)?;
                                    
                                    // Check if it's a reference or owned value
                                    if src.layout.ty.is_ref() {
                                        // It's a reference: read it and follow the pointer
                                        let imm = this.read_immediate(&src)?;
                                        let mplace = this.ref_to_mplace(&imm)?;
                                        let (alloc_id, _, tag) = this.ptr_get_alloc_id(mplace.ptr(), 0)?;
                                        //cleprintln!("Local {:?} (reference) points to allocation {:?} with tag {:?}", local, alloc_id, tag);
                                        
                                        // Update the current_borrower state
                                        if let ProvenanceExtra::Concrete(bor_tag) = tag {
                                            if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                                                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut().current_borrower = bor_tag;
                                                println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
                                            }
                                        }
                                    } else {
                                        // It's an owned value: force it into allocation
                                        let mplace = this.force_allocation(&src)?;
                                        let (alloc_id, _, tag) = this.ptr_get_alloc_id(mplace.ptr(), 0)?;
                                        //println!("Local {:?} (owner) allocated at {:?} with tag {:?}", local, alloc_id, tag);
                                        
                                        // Update the current_borrower state
                                        if let ProvenanceExtra::Concrete(bor_tag) = tag {
                                            if let AllocKind::LiveData = this.get_alloc_info(alloc_id).kind {
                                                this.get_alloc_extra(alloc_id)?.borrow_tracker_hb().borrow_mut().current_borrower = bor_tag;
                                                println!("Updated allocation {:?} current_borrower to {:?}", alloc_id, bor_tag);
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

        interp_ok(())
    }
}

impl<'tcx> EvalContextExt<'tcx> for crate::MiriInterpCx<'tcx> {}