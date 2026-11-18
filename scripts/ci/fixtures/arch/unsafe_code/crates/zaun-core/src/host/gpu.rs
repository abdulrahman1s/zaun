fn f() -> i32 { unsafe { libc::getuid() as i32 } }
