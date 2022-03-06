use libc::{c_int, ucontext_t};

extern "C" {
    // XXX: Deprecated on macOS actually, do not expect this to run.  If it
    // doesn't, use some easy assembly to retrieve the registers.
    pub fn getcontext(ucp: *mut ucontext_t) -> c_int;
}
