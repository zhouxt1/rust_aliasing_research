use crate::borrow_tracker::BorTag;

/// Per-allocation state held by the Hybrid Borrows tracker.
///
/// One instance lives in the alloc-extra of every allocation Miri sees. The fields together
/// describe (a) who is currently allowed to access this memory, (b) what kind of access is
/// permitted, and (c) any raw-pointer-derived borrow chains that have been "exposed" through it.
///
/// Field roles:
/// - `current_borrower` — the tag whose access is currently legal under the standard rules.
/// - `prev_borrower` — kept around in narrow cases (e.g. shortly after a reborrow) so an access
///   through the previous tag can still be accepted. Cleared on the first matching access.
/// - `shared_borrower` — `(tag, refcount)` for the read/frozen permissions; multiple shared
///   borrows share one tag and bump the count.
/// - `exposed_stack` — when an `&mut` is observed through a raw pointer (`&mut *raw`), the
///   chain is recorded here as a stack of `RawPointerStack` entries instead of clobbering
///   `current_borrower`.
/// - `reborrow_chain` — the sequence of tags displaced from `current_borrower` by Ref-source
///   child reborrows (most recently displaced is at the end). Used by `release_protector` to
///   recognize that a protected tag is still a valid ancestor of the effective current borrower
///   even though it is no longer `current_borrower` itself. Cleared whenever `current_borrower`
///   is set by a non-Ref-source mechanism (e.g. `hb_return_mut_borrower`, Frozen→Write).
/// - `perms` — the permission state machine: `Read`, `Write`, `Frozen`, or `Reserved`.
///
/// Status: working for the patterns covered by the `hybrid_alias_tests/` corpus. Known gaps:
/// the GC's `visit_provenance` only walks `current_borrower`, missing `prev_borrower`,
/// `shared_borrower`, and stack tags; non-concrete tag accesses are ignored at the access path.
#[derive(Debug, Clone)]
pub struct BorrowerState {
    pub current_borrower: BorTag,
    pub prev_borrower: Option<BorTag>,
    pub shared_borrower: Option<(BorTag, usize)>, // (tag, count)

    pub exposed_stack: Option<Vec<RawPointerStack>>,

    /// Ancestor tags displaced from `current_borrower` by Ref-source child reborrows.
    /// When `apply_reborrow_to_stack(Ref, ...)` overwrites `current_borrower`, the old value is
    /// pushed here. Consulted by `release_protector` to pass the StrongProtector check when the
    /// effective borrower is a descendant (not an alias) of the protected tag.
    pub reborrow_chain: Vec<BorTag>,

    /// Tags that have been exposed via `expose_provenance` (int-to-ptr laundering) on this
    /// allocation. Almost always 0 or 1 entries in practice, so a `Vec` (no hashing overhead,
    /// no heap allocation until first push) is used instead of a `HashSet`. Consulted by
    /// wildcard memory accesses: a wildcard pointer resolves to whichever live tag is both
    /// still tracked in this `BorrowerState` and present here — see `access` in `mod.rs`.
    pub exposed_tags: Vec<BorTag>,

    pub perms: LocationState,
}

/// Wrapper around the current `BorrowerPermission` for a `BorrowerState`.
///
/// Currently a single-field struct. Kept as its own type so additional per-location state
/// (range info, range-split permissions, protector spans, etc.) can be added without changing
/// every call site that reads or writes the permission.
#[derive(Debug, Clone)]
pub struct LocationState {
    pub permission: BorrowerPermission,
}

/// One frame of the raw-pointer-derived borrow chain.
///
/// When code does `let m = &mut *raw_ptr`, the new mutable borrow `m` is recorded by pushing
/// a `RawPointerStack { base_pointer, current_borrower }` onto `BorrowerState.exposed_stack`,
/// where `base_pointer` is the tag the raw was originally derived from and `current_borrower`
/// is the freshly-minted tag for `m`. Subsequent accesses through `m`, through siblings of
/// `m`, or back through the underlying mutable, are validated by walking this stack.
///
/// Status: working; see `apply_reborrow_to_stack` and `check_raw_pointer_stack` in
/// `hybrid_borrows/mod.rs` for how entries are pushed and consulted.
#[derive(Debug, Clone)]
pub struct RawPointerStack {
    pub base_pointer: BorTag,
    pub current_borrower: BorTag,
}

/// Permission lattice for `BorrowerState`.
///
/// - `Write` — exclusive mutable access via `current_borrower`. Default initial state.
/// - `Read` — one or more live shared borrows; tracked by `shared_borrower`. Reads only.
/// - `Frozen` — all live shared borrows have been "released" but the location remembers the
///   most recent shared tag. A subsequent write upgrades back to `Write` (and clears the
///   shared bookkeeping).
/// - `Reserved` — two-phase mutable borrow that has not yet been activated. Reads through the
///   reserving tag and through `shared_borrower` are both allowed; activation to `Write`
///   happens in `hb_before_terminator` at the call site.
/// - `ReservedIM` — like `Reserved` but the pointee type contains `UnsafeCell` (`!Freeze`).
///   Tolerates writes through `shared_borrower` during the reservation window, because a
///   shared alias of a `Cell`/`RefCell` may legitimately write before the `&mut` activates.
///   Activation by `current_borrower` works identically to `Reserved`.
///
/// Status: working. See the per-permission branches of `check_borrower_tag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowerPermission {
    Read,
    Write,
    Frozen,
    Reserved,
    ReservedIM,
}

impl BorrowerState {
    /// Create a fresh `BorrowerState` rooted at `current_borrower`.
    ///
    /// Used by `BorrowerState::new_allocation` (in `mod.rs`) when an allocation first appears.
    /// Starts in `Write` permission with no prev/shared borrower and no exposed stack.
    ///
    /// Status: working. The initial permission is intentionally `Write` because the freshest
    /// tag for any new allocation has unique access.
    pub fn new(current_borrower: BorTag) -> Self {
        BorrowerState {
            current_borrower,
            prev_borrower: None,
            shared_borrower: None,
            exposed_stack: None,
            reborrow_chain: Vec::new(),
            exposed_tags: Vec::new(),
            perms: LocationState { permission: BorrowerPermission::Write },
        }
    }
}
