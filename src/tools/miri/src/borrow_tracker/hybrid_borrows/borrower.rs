
use crate::borrow_tracker::{BorTag};

#[derive(Debug, Clone)]
pub struct BorrowerState {
    pub current_borrower: BorTag,
    pub prev_borrower: Option<BorTag>,
    pub shared_borrower: Option<(BorTag, usize)>, // (tag, count)

    pub exposed_stack: Option<Vec<BorTag>>,
    pub perms: LocationState,
}

#[derive(Debug, Clone)]
pub struct LocationState {
    pub permission : BorrowerPermission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowerPermission {
    Read,
    Write,
    Frozen,
}

impl BorrowerState {
    pub fn new(current_borrower: BorTag) -> Self {
        BorrowerState { current_borrower, prev_borrower: None, shared_borrower: None, exposed_stack: None, perms: LocationState { permission: BorrowerPermission::Write } }
    }
}