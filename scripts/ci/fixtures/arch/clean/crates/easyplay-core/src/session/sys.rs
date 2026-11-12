#![allow(unsafe_code)]
fn f() { unsafe { libc::getpid() }; }
