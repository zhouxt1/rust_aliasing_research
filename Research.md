# Hybrid Borrows research

This branch is a fork of Rust focused on implementing **Hybrid Borrows**, an alternative aliasing rule in Miri intended as the core of an aliasing sanitizer. It sits in the same design space as Stacked Borrows and Tree Borrows but takes a different position: instead of deriving aliasing decisions purely from runtime structure (a stack or tree of tags), Hybrid Borrows consumes Polonius borrow-checker output and uses it to anchor runtime decisions in the same loan/region facts the compiler already computes statically. The runtime model still tracks per-location borrower state, but transitions are driven by `PoloniusAnchor` MIR statements that are inserted during a custom MIR preparation pass.

The work sits mostly in [src/tools/miri](src/tools/miri), with a smaller set of compiler-side changes used to expose Polonius-derived MIR and metadata that Hybrid Borrows can consume at runtime.

## Documentation index

The detailed research notes live under [docs/](docs/):

- **[docs/architecture.md](docs/architecture.md)** — component-by-component map of the system: driver, MIR preparation pipeline, runtime tracker, sysroot integration, and how a single function flows through them.
- **[docs/build-and-test.md](docs/build-and-test.md)** — how to build with `x.py`, how to run a single test or the full suite, how to skip the std build, and when `core` gets rebuilt.
- **[docs/status.md](docs/status.md)** — snapshot tables of what's working, partial, and not yet implemented in the tracker, the Polonius integration, the driver, and the test corpus.
- **[docs/open-work.md](docs/open-work.md)** — actionable plans for the open items, including the protector implementation roadmap.

The test corpus has its own per-case write-up at [src/tools/miri/tests/hybrid_alias_tests/test_explanation.md](src/tools/miri/tests/hybrid_alias_tests/test_explanation.md).

Function-level documentation lives as doc comments on the functions in [src/tools/miri/src/borrow_tracker/hybrid_borrows/](src/tools/miri/src/borrow_tracker/hybrid_borrows/); the source is the source of truth for what each piece of the tracker does.

## Quick start

```sh
# build everything once (slow)
./x.py build

# inner loop: run one test case under HB, no std sysroot rebuild
MIRI_NO_STD=1 ./x.py run miri --stage 1 \
  --args src/tools/miri/tests/hybrid_alias_tests/pass/test1.rs

# after a compiler-side edit (without a commit), force-rebuild the sysroot
# so stale serialized Polonius MIR for `core` does not shadow your change:
MIRI_FORCE_SYSROOT_REBUILD=1 MIRI_NO_STD=1 \
  ./x.py run miri --stage 1 --args <file>.rs
```

Full details and rationale: [docs/build-and-test.md](docs/build-and-test.md).

## Major next steps

The largest single feature gap is **protector support** — see [docs/open-work.md § Protectors](docs/open-work.md#protectors) for the multi-phase plan. Other open items (diagnostics conversion, suite wiring, driver UX, `ReturnBorrowers` correctness, soundness evaluation) are tracked in the same document.
