//! A "compatibility layer" for spanning XP and Windows 7
//!
//! The standard library currently binds many functions that are not available
//! on Windows XP, but we would also like to support building executables that
//! run on XP. To do this we specify all non-XP APIs as having a fallback
//! implementation to do something reasonable.
//!
//! This dynamic runtime detection of whether a function is available is
//! implemented with `GetModuleHandle` and `GetProcAddress` paired with a
//! static-per-function which caches the result of the first check. In this
//! manner we pay a semi-large one-time cost up front for detecting whether a
//! function is available but afterwards it's just a load and a jump.

use crate::ffi::CString;
use crate::sync::atomic::{AtomicUsize, Ordering};
use crate::sys::c;

pub fn lookup(module: &str, symbol: &str) -> Option<usize> {
    let mut module: Vec<u16> = module.encode_utf16().collect();
    module.push(0);
    let symbol = CString::new(symbol).unwrap();
    unsafe {
        let handle = c::GetModuleHandleW(module.as_ptr());
        match c::GetProcAddress(handle, symbol.as_ptr()) as usize {
            0 => None,
            n => Some(n),
        }
    }
}

pub fn store_func(ptr: &AtomicUsize, module: &str, symbol: &str, fallback: usize) -> usize {
    let value = lookup(module, symbol).unwrap_or(fallback);
    ptr.store(value, Ordering::SeqCst);
    value
}

macro_rules! compat_fn {
    ($module:ident: $(
        $(#[$meta:meta])*
        pub fn $symbol:ident($($argname:ident: $argtype:ty),*)
                                  -> $rettype:ty {
            $($body:expr);*
        }
    )*) => ($(
        #[allow(unused_variables)]
        $(#[$meta])*
        pub unsafe fn $symbol($($argname: $argtype),*) -> $rettype {
            use crate::sync::atomic::{AtomicUsize, Ordering};
            use crate::mem;
            type F = unsafe extern "system" fn($($argtype),*) -> $rettype;

            static PTR: AtomicUsize = AtomicUsize::new(0);

            fn load() -> usize {
                crate::sys::compat::store_func(&PTR,
                                          stringify!($module),
                                          stringify!($symbol),
                                          fallback as usize)
            }

            #[allow(unused_variables)]
            unsafe extern "system" fn fallback($($argname: $argtype),*)
                                               -> $rettype {
                $($body);*
            }

            let addr = match PTR.load(Ordering::SeqCst) {
                0 => load(),
                n => n,
            };
            mem::transmute::<usize, F>(addr)($($argname),*)
        }
    )*)
}

macro_rules! compat_group {
    ($gtype:ident, $gstatic:ident, $gload:ident, $module:ident: $(
        pub fn $symbol:ident($($argname:ident: $argtype:ty),*)
                                  -> $rettype:ty {
            $($body:expr);*
        }
    )*) => (
        struct $gtype {
            $($symbol: crate::sync::atomic::AtomicPtr<()>),*
        }
        static $gstatic: $gtype = $gtype {$(
            $symbol: crate::sync::atomic::AtomicPtr::new({
                //type F = unsafe extern "system" fn($($argtype),*) -> $rettype;

                unsafe extern "system" fn $symbol($($argname: $argtype),*)
                                                  -> $rettype {
                    use self::$symbol;
                    $gload();
                    $symbol($($argname),*)
                }

                $symbol as *mut ()
            })
        ),*};

        fn $gload() {
            use crate::ptr;
            use crate::sync::atomic::{AtomicPtr, Ordering};

            $(
                #[allow(unused_variables)]
                unsafe extern "system" fn $symbol($($argname: $argtype),*)
                                                  -> $rettype {
                    $($body);*
                }
            )*

            const FALLBACKS: $gtype = $gtype {
                $($symbol: AtomicPtr::new($symbol as *mut ())),*
            };

            fn store_funcs(funcs: &$gtype) {
                $($gstatic.$symbol.store(funcs.$symbol.load(Ordering::SeqCst), Ordering::SeqCst);)*
            }

            let funcs: $gtype = $gtype {
                $($symbol: AtomicPtr::new(ptr::null_mut())),*
            };

            $(
                let addr = crate::sys::compat::lookup(stringify!($module), stringify!($symbol));
                match addr {
                    Some(addr) => {
                        funcs.$symbol.store(addr as *mut (), Ordering::SeqCst);
                    },
                    None => {
                        store_funcs(&FALLBACKS);
                        return;
                    }
                }
            )*

            store_funcs(&funcs);
        }

        $(
            #[inline]
            pub unsafe fn $symbol($($argname: $argtype),*) -> $rettype {
                use crate::mem;
                type F = unsafe extern "system" fn($($argtype),*) -> $rettype;

                let addr = $gstatic.$symbol.load(Ordering::SeqCst);
                mem::transmute::<*mut (), F>(addr)($($argname),*)
            }
        )*
    )
}
