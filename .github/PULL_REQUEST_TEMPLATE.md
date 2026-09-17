## What this changes

<!-- One or two sentences. -->

## Effect on output

<!-- Which lines are now dropped or kept that were not before, and why it is
     safe for a model to lose them. Snapshot diffs are the evidence. -->

## Checklist

- [ ] `cargo fmt`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] Snapshot changes reviewed line by line, not blindly accepted
- [ ] Reduction floors in `tests/reduction_test.rs` still pass (updated if the numbers moved)
