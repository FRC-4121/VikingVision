pub mod component_filter;
pub mod components;
pub mod pipeline;
pub mod serialized;
pub mod utils;

pub use vv_camera as camera;
pub use vv_vision::*;

/// Mutexes to be used throughout the crate
///
/// Either a re-export of [`std::sync::Mutex`] or [`no_deadlocks::Mutex`] based
/// on which is enabled.
pub mod mutex {
    #[cfg(feature = "debug-tools")]
    mod inner {
        use no_deadlocks as nd;
        pub use no_deadlocks::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};
        use std::fmt::{self, Debug, Formatter};
        use std::ops::{Deref, DerefMut};
        use std::sync::{LockResult, PoisonError, TryLockError};
        #[derive(Default)]
        pub struct Mutex<T: ?Sized>(nd::Mutex<T>);
        impl<T> Mutex<T> {
            pub fn new(inner: T) -> Self {
                Self(nd::Mutex::new(inner))
            }
            pub fn into_inner(self) -> LockResult<T> {
                self.0.into_inner()
            }
        }
        impl<T: ?Sized> Deref for Mutex<T> {
            type Target = nd::Mutex<T>;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl<T: ?Sized> DerefMut for Mutex<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
        impl<T: Debug> Debug for Mutex<T> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let mut s = f.debug_struct("Mutex");
                match self.try_lock() {
                    Ok(guard) => {
                        s.field("inner", &*guard).field("is_poisoned", &false);
                    }
                    Err(TryLockError::Poisoned(err)) => {
                        s.field("inner", &*err.into_inner())
                            .field("is_poisoned", &true);
                    }
                    Err(TryLockError::WouldBlock) => {}
                }
                s.finish_non_exhaustive()
            }
        }
        #[derive(Default)]
        pub struct RwLock<T: ?Sized>(nd::RwLock<T>);
        impl<T> RwLock<T> {
            pub fn new(inner: T) -> Self {
                Self(nd::RwLock::new(inner))
            }
            pub fn into_inner(self) -> LockResult<T> {
                if self.is_poisoned() {
                    Err(PoisonError::new(self.0.into_inner()))
                } else {
                    Ok(self.0.into_inner())
                }
            }
        }
        impl<T: ?Sized> RwLock<T> {
            pub fn get_mut(&mut self) -> LockResult<&mut T> {
                if self.is_poisoned() {
                    Err(PoisonError::new(self.0.get_mut()))
                } else {
                    Ok(self.0.get_mut())
                }
            }
        }
        impl<T: ?Sized> Deref for RwLock<T> {
            type Target = nd::RwLock<T>;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl<T: ?Sized> DerefMut for RwLock<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
        impl<T: Debug> Debug for RwLock<T> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let mut s = f.debug_struct("RwLock");
                match self.try_read() {
                    Ok(guard) => {
                        s.field("inner", &*guard).field("is_poisoned", &false);
                    }
                    Err(TryLockError::Poisoned(err)) => {
                        s.field("inner", &*err.into_inner())
                            .field("is_poisoned", &true);
                    }
                    Err(TryLockError::WouldBlock) => {}
                }
                s.finish_non_exhaustive()
            }
        }
    }
    #[cfg(feature = "debug-tools")]
    pub use inner::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
    #[cfg(not(feature = "debug-tools"))]
    pub use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
}
