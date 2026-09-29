/**
 * @name Bare multiplication on token amounts
 * @description Unchecked `*` on i128 amounts (e.g. rate * elapsed) can
 *              overflow; use `checked_mul` and handle the `None` case.
 * @kind problem
 * @problem.severity warning
 * @security-severity 5.0
 * @precision low
 * @id rust/soroban/bare-multiplication
 * @tags security
 *       correctness
 *       soroban
 */

import rust

from Expr e, string op
where
  (
    op = e.(BinaryExpr).getOperatorName()
    or
    op = e.(AssignmentOperation).getOperatorName()
  ) and
  op = ["*", "*="] and
  not e.getLocation().getFile().getAbsolutePath().matches(["%/test.rs", "%/tests/%", "%/fuzz/%"])
select e, "Bare '" + op + "' may overflow i128; use checked_mul instead."
