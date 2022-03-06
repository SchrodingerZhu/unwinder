use libc::{c_int, ucontext_t};

extern "C" {
    pub fn getcontext(ucp: *mut ucontext_t) -> c_int;
}
