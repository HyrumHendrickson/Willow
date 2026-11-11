# Contributing

1. Fork, then create a branch.
2. Make the change with tests. Read `AGENTS.md` first; its invariants apply to everyone.
3. Run the checks:

   ```
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

4. Update `CHANGELOG.md` under *Unreleased*.
5. Open a pull request describing what changed and why. Security issues go through `SECURITY.md`, not a pull request.

Contributions are accepted under the MIT license.
