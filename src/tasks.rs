#![allow(clippy::type_complexity)]

use std::cell::UnsafeCell;
use std::collections::VecDeque;

/// A way of spawning tasks within a certain scope.
///
/// This is implemented by [`&std::thread::Scope`], [`rayon::Scope`], and [`rayon::ScopeFifo`], along with
/// the implementors in this module.
pub trait Spawner<'s>: 's {
    /// Spawn a task.
    ///
    /// This should run [`task`] at some point.
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's);
    /// Spawn a task that's a boxed function.
    ///
    /// This delegates to [`Self::spawn`] by default, but can be used to prevent double-boxing.
    fn spawn_boxed(&self, task: Box<dyn FnOnce(&Self) + Send + 's>) {
        self.spawn(task);
    }
}
/// A [`Spawner`] that can spawn tasks that aren't [`Send`].
pub trait LocalSpawner<'s>: Spawner<'s> {
    /// Spawn a task.
    ///
    /// This is like [`Spawner::spawn`], but doesn't require the task be [`Send`].
    fn spawn_local(&self, task: impl FnOnce(&Self) + 's);
    /// Spawn a boxed task.
    ///
    /// This is like [`Spawner::spawn_boxed`], but again, doesn't require that the task be [`Send`].
    /// It delegates to [`Self::spawn_local`] by default.
    fn spawn_local_boxed(&self, task: Box<dyn FnOnce(&Self) + 's>) {
        self.spawn_local(task);
    }
}
impl<'s> Spawner<'s> for rayon::Scope<'s> {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        rayon::Scope::spawn(self, task);
    }
}
impl<'s> Spawner<'s> for rayon::ScopeFifo<'s> {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        self.spawn_fifo(task);
    }
}
impl<'s, 'e> Spawner<'s> for &'s std::thread::Scope<'s, 'e> {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        let this = *self;
        std::thread::Scope::spawn(self, move || task(&this));
    }
}

/// A [`Spawner`] implementation that immediately runs any tasks given to it.
///
/// This is generally not a good idea, because it can lead to unbounded stack usage, but it's made available for testing.
pub struct ImmediatelyRun;
impl<'s> Spawner<'s> for ImmediatelyRun {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        task(self);
    }
}
impl<'s> LocalSpawner<'s> for ImmediatelyRun {
    fn spawn_local(&self, task: impl FnOnce(&Self) + 's) {
        task(self);
    }
}
/// A [`Spawner`] implementation that acts as a FIFO queue.
///
/// This is probably the most useful local local spawner, as it uses a local queue.
#[derive(Default)]
pub struct TaskQueue<'s> {
    queue: UnsafeCell<VecDeque<Box<dyn FnOnce(&Self) + 's>>>,
}
impl<'s> TaskQueue<'s> {
    pub const fn new() -> Self {
        Self {
            queue: UnsafeCell::new(VecDeque::new()),
        }
    }
    pub fn scope<R, F: FnOnce(&Self) -> R>(&mut self, task: F) -> R {
        let ret = task(self);
        while let Some(task) = self.queue.get_mut().pop_front() {
            task(self);
        }
        ret
    }
}
impl<'s> Spawner<'s> for TaskQueue<'s> {
    #[inline(always)]
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        self.spawn_local(task);
    }
    #[inline(always)]
    fn spawn_boxed(&self, task: Box<dyn FnOnce(&Self) + Send + 's>) {
        self.spawn_local_boxed(task);
    }
}
impl<'s> LocalSpawner<'s> for TaskQueue<'s> {
    #[inline(always)]
    fn spawn_local(&self, task: impl FnOnce(&Self) + 's) {
        self.spawn_local_boxed(Box::new(task));
    }
    #[inline(always)]
    fn spawn_local_boxed(&self, task: Box<dyn FnOnce(&Self) + 's>) {
        unsafe {
            (*self.queue.get()).push_back(task);
        }
    }
}

/// A type-erased spawner.
///
/// This borrows from another implementor, but doesn't parameterize over that lifetime because it needs to allow
/// reborrows. Because this could allow a leak if owned, there is no way to get a value of a [`DynSpawner`], only a reference
/// within the closure of [`Self::with`].
pub struct DynSpawner<'s> {
    spawn: fn(*const (), Box<dyn FnOnce(&Self) + Send + 's>),
    spawner: *const (),
}
unsafe impl Sync for DynSpawner<'_> {}
impl<'s> DynSpawner<'s> {
    fn spawn_impl<S: Spawner<'s> + Send + Sync>(
        spawn_ptr: *const (),
        task: Box<dyn FnOnce(&Self) + Send + 's>,
    ) {
        let spawner_impl = unsafe { &*(spawn_ptr as *const S) };
        spawner_impl.spawn(move |spawner| Self::with::<S, _, _>(spawner, task));
    }
    /// Call a closure with a type-erased spawner available.
    ///
    /// [`DynSpawner`]'s type signature doesn't include the lifetime of the spawner (it can't, or else reborrowing wouldn't work),
    /// so the closure can't escape this borrowed scope.
    pub fn with<S: Spawner<'s> + Send + Sync, R, F: FnOnce(&Self) -> R>(spawner: &S, f: F) -> R {
        let this = Self {
            spawn: Self::spawn_impl::<S>,
            spawner: spawner as *const S as *const (),
        };
        f(&this)
    }
}
impl<'s> Spawner<'s> for DynSpawner<'s> {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        self.spawn_boxed(Box::new(task));
    }
    fn spawn_boxed(&self, task: Box<dyn FnOnce(&Self) + Send + 's>) {
        (self.spawn)(self.spawner, task);
    }
}

/// A type-erased spawner.
///
/// This borrows from another implementor, but doesn't parameterize over that lifetime because it needs to allow
/// reborrows. Because this could allow a leak if owned, there is no way to get a value of a [`DynLocalSpawner`], only a
/// reference within the closure of [`Self::with`].
pub struct DynLocalSpawner<'s> {
    spawn: fn(*const (), Box<dyn FnOnce(&Self) + 's>),
    spawner: *const (),
}
impl<'s> DynLocalSpawner<'s> {
    /// Call a closure with a type-erased spawner available.
    ///
    /// [`DynLocalSpawner`]'s type signature doesn't include the lifetime of the spawner (it can't, or else reborrowing
    /// wouldn't work), so the closure can't escape this borrowed scope.
    ///
    /// This function assumes that the behavior of [`S::spawn_local`] is the same as [`S::spawn`]. It's possible to create
    /// a specializing spawner that doesn't make this assumption (and is also [`Sync`]), but I saw no need for it with the
    /// current spawners.
    pub fn with<S: LocalSpawner<'s>, R, F: FnOnce(&Self) -> R>(spawner: &S, f: F) -> R {
        fn spawn_impl<'s, S: LocalSpawner<'s>>(
            spawn_ptr: *const (),
            task: Box<dyn FnOnce(&DynLocalSpawner<'s>) + 's>,
        ) {
            let spawner_impl = unsafe { &*(spawn_ptr as *const S) };
            spawner_impl
                .spawn_local(move |spawner| DynLocalSpawner::<'s>::with::<S, _, _>(spawner, task));
        }
        let this = Self {
            spawn: spawn_impl::<S>,
            spawner: spawner as *const S as *const (),
        };
        f(&this)
    }
}
impl<'s> Spawner<'s> for DynLocalSpawner<'s> {
    fn spawn(&self, task: impl FnOnce(&Self) + Send + 's) {
        self.spawn_boxed(Box::new(task));
    }
    fn spawn_boxed(&self, task: Box<dyn FnOnce(&Self) + Send + 's>) {
        self.spawn_local_boxed(task);
    }
}
impl<'s> LocalSpawner<'s> for DynLocalSpawner<'s> {
    fn spawn_local(&self, task: impl FnOnce(&Self) + 's) {
        self.spawn_local_boxed(Box::new(task));
    }
    fn spawn_local_boxed(&self, task: Box<dyn FnOnce(&Self) + 's>) {
        (self.spawn)(self.spawner, task);
    }
}
