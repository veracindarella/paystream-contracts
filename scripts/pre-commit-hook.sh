#!/bin/bash
# Pre-commit hook for PayStream contracts
# Runs cargo fmt --check and cargo clippy before allowing commits
# Skip this hook with: git commit --no-verify

set -e

echo "Running pre-commit checks..."

# Check formatting
echo "Checking code formatting with cargo fmt..."
if ! cargo fmt --check; then
    echo "❌ Code formatting check failed."
    echo "Run 'cargo fmt' to fix formatting issues."
    exit 1
fi
echo "✓ Code formatting check passed."

# Run clippy
echo "Running clippy lints..."
if ! cargo clippy --all-targets -- -D warnings; then
    echo "❌ Clippy check failed."
    echo "Fix the warnings above before committing."
    exit 1
fi
echo "✓ Clippy check passed."

echo "All pre-commit checks passed!"
