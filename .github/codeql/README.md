# Custom CodeQL queries

No published CodeQL query pack targets Soroban yet (GitHub's `codeql/rust-queries`
covers generic Rust only), so this directory ships a small local pack that runs
alongside the default suite via `codeql-config.yml`.

| Query | Id | Detects |
|---|---|---|
| `queries/MissingRequireAuth.ql` | `rust/soroban/missing-require-auth` | Public contract methods that write storage without `require_auth` |
| `queries/BareMultiplication.ql` | `rust/soroban/bare-multiplication` | Unchecked `*` / `*=` on amounts (i128 overflow risk) |

Run the true-positive tests locally with the CodeQL CLI:

```bash
codeql pack install .github/codeql/queries
codeql pack install .github/codeql/tests
codeql test run .github/codeql/tests
```
