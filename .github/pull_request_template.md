## What and why

<!-- What does this change, and what problem does it solve? Link the issue if there is one. -->

## Testing

- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
- [ ] `.\test.ps1` passes (or `.\test.ps1 -NoGame`, with the reason the game test couldn't run)
- Factorio version tested:

## Checklist

- [ ] Hooks still always call the original function and can't let a panic reach the game
- [ ] `CHANGELOG.md` has an entry under `Unreleased` for user-visible changes
