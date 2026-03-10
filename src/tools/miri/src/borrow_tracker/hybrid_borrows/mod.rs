use rustc_abi::Size;
use rustc_data_structures::fx::FxHashSet;
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
        prov_extra: ProvenanceExtra,
        _range: AllocRange,
        machine: &MiriMachine<'tcx>,
    ) -> InterpResult<'tcx> {
        let location = machine.threads.active_thread_stack().last().map(|frame| frame.current_loc());

        println!("Memory access at location: {:?}", location);

        if let ProvenanceExtra::Concrete(tag) = prov_extra {
            self.current_borrower = tag;
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

impl VisitProvenance for AllocState {
    fn visit_provenance(&self, visit: &mut VisitWith<'_>) {
        visit(None, Some(self.current_borrower));
    }
}

pub trait EvalContextExt<'tcx>: crate::MiriInterpCxExt<'tcx> {
    fn hb_retag_ptr_value(
        &mut self,
        _kind: RetagKind,
        val: &ImmTy<'tcx>,
    ) -> InterpResult<'tcx, ImmTy<'tcx>> {
        interp_ok(val.clone())
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
}

impl<'tcx> EvalContextExt<'tcx> for crate::MiriInterpCx<'tcx> {}