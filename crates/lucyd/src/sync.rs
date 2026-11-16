use std::sync::{Mutex, MutexGuard};

pub(crate) const POISONED: &str = "no thread panics while holding a lock";

pub(crate) trait Locked<T: ?Sized> {
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> Locked<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().expect(POISONED)
    }
}
