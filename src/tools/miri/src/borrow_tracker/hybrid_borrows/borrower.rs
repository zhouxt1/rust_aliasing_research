use crate::borrow_tracker::BorTag;

/// Exposed stack is used when there is a reference derived from a mutable borrow.
/// It tracks a few things. Root pointer, its unique current borrower.
///
#[derive(Debug, Clone)]
pub struct BorrowerState {
    pub current_borrower: BorTag,
    pub prev_borrower: Option<BorTag>,
    pub shared_borrower: Option<(BorTag, usize)>, // (tag, count)

    pub exposed_stack: Option<Vec<RawPointerStack>>,

    pub perms: LocationState,
}

#[derive(Debug, Clone)]
pub struct LocationState {
    pub permission: BorrowerPermission,
}

#[derive(Debug, Clone)]
pub struct RawPointerStack {
    pub base_pointer: BorTag,
    pub current_borrower: BorTag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowerPermission {
    Read,
    Write,
    Frozen,
    Reserved,
}

impl BorrowerState {
    pub fn new(current_borrower: BorTag) -> Self {
        BorrowerState {
            current_borrower,
            prev_borrower: None,
            shared_borrower: None,
            exposed_stack: None,
            perms: LocationState { permission: BorrowerPermission::Write },
        }
    }
}
