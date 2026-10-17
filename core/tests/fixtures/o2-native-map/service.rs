pub struct User { pub id: u64 }
pub trait Store { fn lookup(&self, id: u64) -> Option<User>; }
pub struct Service<T: Store> { store: T }
impl<T: Store> Service<T> {
    /// lookup returns a source-backed record.
    pub fn lookup(&self, id: u64) -> Option<User> {
        self.store.lookup(id)
    }
}
pub mod api {
    pub fn sync() -> bool { true }
}
const EXAMPLE: &str = "fn lookup() is documentation, not a declaration";
