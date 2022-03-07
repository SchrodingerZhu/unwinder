pub use libc::*;

#[cfg(target_os = "macos")]
extern "C" {
    // XXX: Deprecated on macOS actually, do not expect this to run.  If it
    // doesn't, use some easy assembly to retrieve the registers.
    pub fn getcontext(ucp: *mut libc::ucontext_t) -> libc::c_int;
}
