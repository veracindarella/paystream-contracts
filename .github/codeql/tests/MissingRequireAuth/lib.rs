pub struct Address;
impl Address {
    pub fn require_auth(&self) {}
}
fn set_admin(_a: &Address) {}

pub struct Contract;
impl Contract {
    pub fn bad_set_admin(admin: Address) {
        set_admin(&admin);
    }
    pub fn good_set_admin(admin: Address) {
        admin.require_auth();
        set_admin(&admin);
    }
}
