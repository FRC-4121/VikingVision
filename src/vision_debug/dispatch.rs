use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

struct Storage {
    buffers: [UnsafeCell<boxcar::Vec<MaybeUninit<super::DebugImage>>>; 2],
    refcounts: [AtomicU16; 2],
    write_to: AtomicBool,
}

#[derive(Clone)]
pub struct Writer {
    inner: Arc<Storage>,
}
unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}
impl Writer {
    pub fn write(&self, message: super::DebugImage) -> bool {
        loop {
            let idx = self.inner.write_to.load(Ordering::Acquire) as usize;
            let old = self.inner.refcounts[idx].fetch_add(1, Ordering::AcqRel);
            debug_assert_ne!(old, u16::MAX, "refcount overflow");
            if self.inner.write_to.load(Ordering::Acquire) as usize == idx {
                // SAFETY: we only access this through a shared reference
                let new =
                    unsafe { (*self.inner.buffers[idx].get()).push(MaybeUninit::new(message)) };
                self.inner.refcounts[idx].fetch_sub(1, Ordering::AcqRel);
                return new == 0; // check if this was the first element added
            } else {
                self.inner.refcounts[idx].fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

pub struct Reader {
    inner: Arc<Storage>,
}
unsafe impl Send for Reader {}
impl Reader {
    pub fn drain(&self, mut f: impl FnMut(super::DebugImage)) {
        let idx = self.inner.write_to.fetch_not(Ordering::AcqRel) as usize;
        // SAFETY: for now, we only have a shared reference
        let vec = unsafe { &(*self.inner.buffers[idx].get()) };
        while self.inner.refcounts[idx].load(Ordering::Acquire) != 0 {
            std::hint::spin_loop(); // this is just in case we started draining mid-write
        }
        let mut it = vec.iter();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (_, elem) in &mut it {
                // SAFETY: we only push initialized messages
                unsafe {
                    f(elem.assume_init_read());
                }
            }
        }));
        // make sure we drop the rest
        // SAFETY: we only push initialized images
        it.for_each(|(_, elem)| unsafe { drop(elem.assume_init_read()) });
        unsafe {
            (*self.inner.buffers[idx].get()).clear();
        }
        if let Err(payload) = res {
            std::panic::resume_unwind(payload);
        }
    }
}

pub fn pair() -> (Writer, Reader) {
    #[allow(clippy::arc_with_non_send_sync)]
    let inner = Arc::new(Storage {
        buffers: [
            UnsafeCell::new(boxcar::Vec::new()),
            UnsafeCell::new(boxcar::Vec::new()),
        ],
        refcounts: [AtomicU16::new(0), AtomicU16::new(0)],
        write_to: AtomicBool::new(false),
    });
    (
        Writer {
            inner: Arc::clone(&inner),
        },
        Reader { inner },
    )
}
