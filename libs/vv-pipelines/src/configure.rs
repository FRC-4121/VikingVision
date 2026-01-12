use std::cell::UnsafeCell;
use std::fmt::Display;
use std::mem::ManuallyDrop;
use std::sync::Once;

pub trait Configure<C, S, A> {
    fn name(&self) -> impl Display {
        disqualified::ShortName::of::<Self>()
    }
    fn configure(&self, config: C, arg: A) -> S;
}
union ConfigurableInner<C, S> {
    config: ManuallyDrop<C>,
    state: ManuallyDrop<S>,
}
pub struct Configurable<C, S, T> {
    inner: UnsafeCell<ConfigurableInner<C, S>>,
    once: Once,
    def: T,
}
unsafe impl<C: Sync, S: Sync, T: Sync> Sync for Configurable<C, S, T> {}
impl<C, S, T> Drop for Configurable<C, S, T> {
    fn drop(&mut self) {
        unsafe {
            let mut drop_state = true;
            self.once.call_once_force(|s| {
                drop_state = false;
                if !s.is_poisoned() {
                    ManuallyDrop::drop(&mut self.inner.get_mut().config);
                }
            });
            if drop_state {
                ManuallyDrop::drop(&mut self.inner.get_mut().state);
            }
        }
    }
}
impl<C, S, T> Configurable<C, S, T> {
    pub const fn new(config: C, def: T) -> Self {
        Self {
            once: Once::new(),
            inner: UnsafeCell::new(ConfigurableInner {
                config: ManuallyDrop::new(config),
            }),
            def,
        }
    }
    pub fn get_config(&self) -> Option<&C> {
        self.once
            .is_completed()
            .then(|| unsafe { &*(*self.inner.get()).config })
    }
    pub fn get_state(&self) -> Option<&S> {
        if self.once.is_completed() {
            Some(unsafe { &(*self.inner.get()).state })
        } else {
            tracing::error!(
                "tried to get data from an uninitialized {}",
                disqualified::ShortName::of::<T>()
            );
            None
        }
    }
    pub fn init<A>(&self, arg: A) -> bool
    where
        T: Configure<C, S, A>,
    {
        let mut ran = false;
        self.once.call_once(|| unsafe {
            let config = ManuallyDrop::take(&mut (*self.inner.get()).config);
            let state = self.def.configure(config, arg);
            (*self.inner.get()).state = ManuallyDrop::new(state);
            ran = true;
        });
        ran
    }
}
impl<C, S, T> Configurable<C, Option<S>, T> {
    /// Convenience function to flatten an `Option` state, since there isn't really a nice way to chain it.
    #[inline(always)]
    pub fn get_state_flat(&self) -> Option<&S> {
        self.get_state()?.as_ref()
    }
}
