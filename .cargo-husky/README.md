# Git Hooks

This directory contains git hooks for the preconfirmation-gateway project.

## Installation

To install the pre-commit hook, copy it to your local `.git/hooks/` directory:

```bash
cp .cargo-husky/hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

Or use this one-liner from the repository root:

```bash
cp .cargo-husky/hooks/pre-commit .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
```

## Pre-commit Hook

The pre-commit hook automatically:

1. **Formats code** with `cargo fmt --all`
2. **Fails the commit** if formatting changes files (so you can stage them)
3. **Runs Clippy** to catch common issues
4. **Blocks the commit** if Clippy finds warnings or errors

### Why the hook fails on formatting changes

If `cargo fmt` modifies files, the hook will fail with:

```
❌ cargo fmt modified files. Please stage the changes and commit again:
src/some/file.rs
```

This ensures formatting changes are always included in the commit, not left as uncommitted changes.

To fix this:
1. Stage the formatted files: `git add .`
2. Commit again: `git commit`

## Manual Setup (Alternative)

If you prefer to manually install hooks, you can also:

```bash
# Copy individual hook files
cp .cargo-husky/hooks/pre-commit .git/hooks/
chmod +x .git/hooks/pre-commit
```

## Note on cargo-husky

This project previously used `cargo-husky` but has removed it as a dependency. The hooks are now managed manually by copying them from `.cargo-husky/hooks/` to `.git/hooks/`.
