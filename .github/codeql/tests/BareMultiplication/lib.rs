pub fn accrued(rate: i128, elapsed: i128) -> i128 {
    rate * elapsed
}
pub fn accrued_checked(rate: i128, elapsed: i128) -> Option<i128> {
    rate.checked_mul(elapsed)
}
