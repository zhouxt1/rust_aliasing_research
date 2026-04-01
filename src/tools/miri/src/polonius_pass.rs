use rustc_data_structures::graph::dominators::Dominators;
use rustc_middle::mir::{
    self, BasicBlock, BasicBlockData, Body, ConstOperand, Operand,
    PoloniusAnchorData, PoloniusAnchorId, PoloniusAnchorKind, Statement, StatementKind,
    TerminatorKind,
};
use rustc_middle::ty::{self, TyCtxt};
use rustc_hir as hir;
use rustc_mir_transform::patch::MirPatch;
use rustc_span::source_map::Spanned;
use rustc_span::{DUMMY_SP, sym};

use crate::machine::PoloniusFacts;

fn next_anchor_id(body: &Body<'_>) -> PoloniusAnchorId {
    body.polonius_anchor_data.keys().copied().max().map_or(0, |id| id + 1)
}

// fn rewrite_false_edges<'tcx>(body: &mut Body<'tcx>) {
//     let mut invalidate_cfg = false;

//     for basic_block in body.basic_blocks.as_mut_preserves_cfg().iter_mut() {
//         let terminator = basic_block.terminator_mut();
//         match terminator.kind {
//             TerminatorKind::FalseEdge { real_target, .. }
//             | TerminatorKind::FalseUnwind { real_target, .. } => {
//                 invalidate_cfg = true;
//                 terminator.kind = TerminatorKind::Goto { target: real_target };
//             }
//             _ => {}
//         }
//     }

//     if invalidate_cfg {
//         body.basic_blocks.invalidate_cfg_cache();
//     }
// }

fn add_return_borrower_anchors_with_patch<'tcx>(body: &mut Body<'tcx>, facts: &PoloniusFacts<'tcx>) {
    let mut pending = Vec::new();

    for (&location, return_borrowers) in &facts.return_borrowers {
        if let Some(locals) = &return_borrowers.mut_borrows
            && !locals.is_empty()
        {
            pending.push((
                location,
                PoloniusAnchorKind::MutReturnBorrower { locals: locals.clone() },
            ));
        }

        if let Some(locals) = &return_borrowers.shared
            && !locals.is_empty()
        {
            pending.push((
                location,
                PoloniusAnchorKind::SharedReturnVar { locals: locals.clone() },
            ));
        }

        if let Some(locals) = &return_borrowers.two_phase
            && !locals.is_empty()
        {
            pending.push((
                location,
                PoloniusAnchorKind::TwoPhaseReturnBorrower { locals: locals.clone() },
            ));
        }
    }

    pending.sort_by_key(|(location, _)| (location.block.as_usize(), location.statement_index));

    let mut patch = MirPatch::new(body);
    let mut next_id = next_anchor_id(body);

    for (location, kind) in pending {
        let anchor_id = next_id;
        next_id += 1;

        body.polonius_anchor_data.insert(anchor_id, PoloniusAnchorData { kind });
        patch.add_statement(location, StatementKind::PoloniusAnchor(anchor_id));
    }

    patch.apply(body);
}

fn remap_mir_for_const_eval_select<'tcx>(
    tcx: TyCtxt<'tcx>,
    mut body: Body<'tcx>,
    context: hir::Constness,
) -> Body<'tcx> {
    for bb in body.basic_blocks.as_mut().iter_mut() {
        let terminator = bb.terminator.as_mut().expect("invalid terminator");
        match terminator.kind {
            TerminatorKind::Call {
                func: Operand::Constant(ref func),
                ref mut args,
                destination,
                target,
                unwind,
                fn_span,
                ..
            } if let ConstOperand { ref const_, .. } = **func
                && let ty::FnDef(def_id, _) = *const_.ty().kind()
                && tcx.is_intrinsic(def_id, sym::const_eval_select) =>
            {
                let Ok([tupled_args, called_in_const, called_at_rt]) =
                    take_array(args)
                else {
                    unreachable!()
                };
                let ty = tupled_args.node.ty(&body.local_decls, tcx);
                let fields = ty.tuple_fields();
                let num_args = fields.len();
                let func =
                    if context == hir::Constness::Const { called_in_const } else { called_at_rt };
                let (method, place): (fn(mir::Place<'tcx>) -> Operand<'tcx>, mir::Place<'tcx>) =
                    match tupled_args.node {
                        Operand::Constant(_) | Operand::RuntimeChecks(_) => {
                            let local = body.local_decls.push(mir::LocalDecl::new(ty, fn_span));
                            bb.statements.push(Statement::new(
                                mir::SourceInfo::outermost(fn_span),
                                StatementKind::Assign(Box::new((
                                    local.into(),
                                    mir::Rvalue::Use(tupled_args.node.clone()),
                                ))),
                            ));
                            (Operand::Move, local.into())
                        }
                        Operand::Move(place) => (Operand::Move, place),
                        Operand::Copy(place) => (Operand::Copy, place),
                    };
                let place_elems = place.projection;
                let untupled_args = (0..num_args)
                    .map(|idx| {
                        let mut place_elems = place_elems.iter().collect::<Vec<_>>();
                        place_elems.push(mir::ProjectionElem::Field(idx.into(), fields[idx]));
                        let projection = tcx.mk_place_elems(&place_elems);
                        let place = mir::Place { local: place.local, projection };
                        Spanned { node: method(place), span: DUMMY_SP }
                    })
                    .collect();

                terminator.kind = TerminatorKind::Call {
                    func: func.node,
                    args: untupled_args,
                    destination,
                    target,
                    unwind,
                    call_source: mir::CallSource::Misc,
                    fn_span,
                };
            }
            _ => {}
        }
    }

    body
}

fn take_array<T, const N: usize>(b: &mut Box<[T]>) -> Result<[T; N], Box<[T]>> {
    let b: Box<[T; N]> = std::mem::take(b).try_into()?;
    Ok(*b)
}

fn has_back_edge(
    doms: &Dominators<BasicBlock>,
    node: BasicBlock,
    node_data: &BasicBlockData<'_>,
) -> bool {
    if !doms.is_reachable(node) {
        return false;
    }
    node_data.terminator().successors().any(|succ| doms.dominates(succ, node))
}

fn apply_ctfe_limit<'tcx>(body: &mut Body<'tcx>) {
    let doms = body.basic_blocks.dominators();
    let indices: Vec<BasicBlock> = body
        .basic_blocks
        .iter_enumerated()
        .filter_map(|(node, node_data)| {
            if matches!(
                node_data.terminator().kind,
                TerminatorKind::Call { .. } | TerminatorKind::TailCall { .. }
            ) || has_back_edge(&doms, node, node_data)
            {
                Some(node)
            } else {
                None
            }
        })
        .collect();

    let basic_blocks = body.basic_blocks.as_mut_preserves_cfg();
    for index in indices {
        let bbdata = &mut basic_blocks[index];
        let source_info = bbdata.terminator().source_info;
        bbdata
            .statements
            .push(Statement::new(source_info, StatementKind::ConstEvalCounter));
    }
}

pub(crate) fn prepare_polonius_mir_for_miri<'tcx>(
    tcx: TyCtxt<'tcx>,
    facts: &PoloniusFacts<'tcx>,
) -> Body<'tcx> {
    let mut body = facts.body.clone();
 
    // rewrite_false_edges(&mut body);
    add_return_borrower_anchors_with_patch(&mut body, facts);
    rustc_mir_transform::run_analysis_to_runtime_passes(tcx, &mut body);
    body = remap_mir_for_const_eval_select(tcx, body, hir::Constness::Const);
    //apply_ctfe_limit(&mut body);

    body
}
