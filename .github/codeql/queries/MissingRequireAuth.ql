/**
 * @name Missing require_auth on state-mutating Soroban function
 * @description A public contract method writes contract storage without
 *              calling `require_auth`, so any caller can mutate state.
 * @kind problem
 * @problem.severity warning
 * @security-severity 7.5
 * @precision medium
 * @id rust/soroban/missing-require-auth
 * @tags security
 *       soroban
 */

import rust

/** Holds if `f` contains a method or function call whose name matches `pattern`. */
predicate callsMatching(Function f, string pattern) {
  exists(MethodCallExpr mc |
    mc.getEnclosingCallable() = f and
    mc.getIdentifier().getText().regexpMatch(pattern)
  )
  or
  exists(CallExpr ce |
    ce.getEnclosingCallable() = f and
    ce.getFunction().toString().regexpMatch("(.*::)?" + pattern)
  )
}

/** Storage writes: `storage().x().set/remove(..)` or `set_*`/`save_*`/`remove_*` helpers. */
predicate mutatesState(Function f) { callsMatching(f, "(set|remove|save|store|put)(_.*)?") }

/** Authorization: `require_auth`, `require_auth_for_args`, or `*auth*`/`*admin*` guard helpers. */
predicate checksAuth(Function f) { callsMatching(f, ".*(auth|require_admin).*") }

from Function f
where
  // public methods inside an `impl` block (i.e. `#[contractimpl]` entry points)
  exists(Impl i | f = i.getAssocItemList().getAnAssocItem()) and
  exists(f.getVisibility()) and
  mutatesState(f) and
  not checksAuth(f)
select f,
  "Contract function '" + f.getName().getText() +
    "' mutates storage without calling require_auth."
