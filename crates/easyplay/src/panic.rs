//! Panic hook: log the panic with a backtrace, then run the default hook. Crash markers
//! (`<state>/crash/*.json`, LOGGING §6) are task ERR-005. Never kills sessions: they belong to
//! their supervisors.

/// Install the hook.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            code = "EP-INT-002",
            thread = thread.name().unwrap_or("unnamed"),
            "panic: {info}\n{backtrace}"
        );
        default(info);
    }));
}
