//! System Mutexes
//!
//! The Windows implementation of mutexes is a little odd and it might not be
//! immediately obvious what's going on. The primary oddness is that SRWLock is
//! used instead of CriticalSection, and this is done because:
//!
//! 1. SRWLock is several times faster than CriticalSection according to
//!    benchmarks performed on both Windows 8 and Windows 7.
//!
//! 2. CriticalSection allows recursive locking while SRWLock deadlocks. The
//!    Unix implementation deadlocks so consistency is preferred. See #19962 for
//!    more details.
//!
//! 3. While CriticalSection is fair and SRWLock is not, the current Rust policy
//!    is that there are no guarantees of fairness.

use crate::cell::UnsafeCell;
use crate::mem::{self, MaybeUninit};
use crate::sys::c;
use crate::sys::rwlock::{self, RWLock};

pub struct Mutex {
    inner: RWLock,
}

// Windows SRW Locks are movable (while not borrowed).
pub type MovableMutex = Mutex;

unsafe impl Send for Mutex {}
unsafe impl Sync for Mutex {}

#[inline]
pub unsafe fn raw_srw(m: &Mutex) -> c::PSRWLOCK {
    rwlock::raw_srw(&m.inner)
}

#[inline]
pub unsafe fn raw_cs(m: &Mutex) -> c::PCRITICAL_SECTION {
    let remutex = (*m.inner.rerwlock()).remutex();

    debug_assert!(mem::size_of::<c::CRITICAL_SECTION>() <= mem::size_of_val(&(*remutex).inner));
    &remutex.inner as *const _ as *mut _
}

impl Mutex {
    pub const fn new() -> Mutex {
        Mutex { inner: RWLock::new() }
    }

    #[inline]
    pub unsafe fn init(&mut self) {}

    #[inline]
    pub unsafe fn lock(&self) {
        self.inner.write();
    }

    #[inline]
    pub unsafe fn try_lock(&self) -> bool {
        self.inner.try_write()
    }

    #[inline]
    pub unsafe fn unlock(&self) {
        self.inner.write_unlock();
    }

    #[inline]
    pub unsafe fn destroy(&self) {
        self.inner.destroy();
    }
}

pub struct ReentrantMutex {
    inner: MaybeUninit<UnsafeCell<c::CRITICAL_SECTION>>,
}

unsafe impl Send for ReentrantMutex {}
unsafe impl Sync for ReentrantMutex {}

impl ReentrantMutex {
    pub const fn uninitialized() -> ReentrantMutex {
        ReentrantMutex { inner: MaybeUninit::uninit() }
    }

    pub unsafe fn init(&self) {
        c::InitializeCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    pub unsafe fn lock(&self) {
        c::EnterCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    #[inline]
    pub unsafe fn try_lock(&self) -> bool {
        c::TryEnterCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr())) != 0
    }

    pub unsafe fn unlock(&self) {
        c::LeaveCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }

    pub unsafe fn destroy(&self) {
        c::DeleteCriticalSection(UnsafeCell::raw_get(self.inner.as_ptr()));
    }
}
