use crate::cell::UnsafeCell;
use crate::mem;
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::c;
use crate::sys::mutex::ReentrantMutex;

pub struct RWLock {
    inner: AtomicUsize,
}

pub type MovableRWLock = RWLock;

unsafe impl Send for RWLock {}
unsafe impl Sync for RWLock {}

#[derive(Clone, Copy)]
pub enum Kind {
    SRWLock = 1,
    CriticalSection = 2,
}

#[inline]
pub unsafe fn raw_srw(lock: &RWLock) -> c::PSRWLOCK {
    debug_assert!(mem::size_of::<c::SRWLOCK>() <= mem::size_of_val(&lock.inner));
    &lock.inner as *const _ as *mut _
}

impl RWLock {
    pub const fn new() -> RWLock {
        RWLock {
            // This works because SRWLOCK_INIT is 0 (wrapped in a struct), so we are also properly
            // initializing an SRWLOCK here.
            inner: AtomicUsize::new(0),
        }
    }

    #[inline]
    pub unsafe fn read(&self) {
        match kind() {
            Kind::SRWLock => c::AcquireSRWLockShared(raw_srw(self)),
            Kind::CriticalSection => (*self.rerwlock()).read(),
        }
    }

    #[inline]
    pub unsafe fn try_read(&self) -> bool {
        match kind() {
            Kind::SRWLock => c::TryAcquireSRWLockShared(raw_srw(self)) != 0,
            Kind::CriticalSection => (*self.rerwlock()).try_read(),
        }
    }

    #[inline]
    pub unsafe fn write(&self) {
        match kind() {
            Kind::SRWLock => c::AcquireSRWLockExclusive(raw_srw(self)),
            Kind::CriticalSection => (*self.rerwlock()).write(),
        }
    }

    #[inline]
    pub unsafe fn try_write(&self) -> bool {
        match kind() {
            Kind::SRWLock => c::TryAcquireSRWLockExclusive(raw_srw(self)) != 0,
            Kind::CriticalSection => (*self.rerwlock()).try_write(),
        }
    }

    #[inline]
    pub unsafe fn read_unlock(&self) {
        match kind() {
            Kind::SRWLock => c::ReleaseSRWLockShared(raw_srw(self)),
            Kind::CriticalSection => (*self.rerwlock()).read_unlock(),
        }
    }

    #[inline]
    pub unsafe fn write_unlock(&self) {
        match kind() {
            Kind::SRWLock => c::ReleaseSRWLockExclusive(raw_srw(self)),
            Kind::CriticalSection => (*self.rerwlock()).write_unlock(),
        }
    }

    #[inline]
    pub unsafe fn destroy(&self) {
        match kind() {
            Kind::SRWLock => {}
            Kind::CriticalSection => (*self.rerwlock()).destroy(),
        }
    }

    pub(super) unsafe fn rerwlock(&self) -> *mut ReentrantRWLock {
        match self.inner.load(Ordering::SeqCst) {
            0 => {}
            n => return n as *mut _,
        }
        let mut m = box ReentrantRWLock::uninitialized();
        m.init();
        let m = Box::into_raw(m);
        match self.inner.compare_exchange(0, m as usize, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => m,
            Err(n) => {
                Box::from_raw(m).destroy();
                n as *mut _
            }
        }
    }
}

pub fn kind() -> Kind {
    match c::AcquireSRWLockExclusive::option() {
        Some(_) => Kind::SRWLock,
        None => Kind::CriticalSection,
    }
}

pub struct ReentrantRWLock {
    inner: ReentrantMutex,
    reader_count: UnsafeCell<usize>,
    write_held: UnsafeCell<bool>,
}

unsafe impl Send for ReentrantRWLock {}
unsafe impl Sync for ReentrantRWLock {}

impl ReentrantRWLock {
    pub fn uninitialized() -> ReentrantRWLock {
        ReentrantRWLock {
            inner: ReentrantMutex::uninitialized(),
            reader_count: UnsafeCell::new(0),
            write_held: UnsafeCell::new(false),
        }
    }

    pub unsafe fn init(&mut self) {
        self.inner.init();

        self.inner.lock();
        *self.reader_count.get() = 0;
        *self.write_held.get() = false;
        self.inner.unlock();
    }

    pub unsafe fn read(&self) {
        self.inner.lock();

        if *self.write_held.get() {
            self.inner.unlock();
            panic!("tried to acquire read lock after acquiring write lock");
        }

        *self.reader_count.get() += 1;
    }

    pub unsafe fn try_read(&self) -> bool {
        if !self.inner.try_lock() {
            false
        } else if *self.write_held.get() {
            self.inner.unlock();
            false
        } else {
            *self.reader_count.get() += 1;
            true
        }
    }

    pub unsafe fn write(&self) {
        self.inner.lock();

        if *self.write_held.get() {
            self.inner.unlock();
            panic!("tried to acquire write lock on already acquired write lock");
        }

        *self.write_held.get() = true;
    }

    pub unsafe fn try_write(&self) -> bool {
        if !self.inner.try_lock() {
            false
        } else if *self.write_held.get() || *self.reader_count.get() > 0 {
            self.inner.unlock();
            false
        } else {
            *self.write_held.get() = true;
            true
        }
    }

    pub unsafe fn read_unlock(&self) {
        if *self.reader_count.get() == 0 {
            // FIXME: have some kind of poison flag?
            self.inner.unlock();
            panic!("tried to unlock reader lock with no readers");
        }

        *self.reader_count.get() -= 1;

        self.inner.unlock();
    }

    pub unsafe fn write_unlock(&self) {
        if !*self.write_held.get() {
            // FIXME: have some kind of poison flag?
            self.inner.unlock();
            panic!("tried to unlock write lock with no writer");
        }

        *self.write_held.get() = false;

        self.inner.unlock();
    }

    pub unsafe fn destroy(&self) {
        self.inner.lock();
        *self.reader_count.get() = 0;
        *self.write_held.get() = false;
        self.inner.unlock();

        self.inner.destroy();
    }

    pub(super) fn remutex(&self) -> &ReentrantMutex {
        &self.inner
    }
}
