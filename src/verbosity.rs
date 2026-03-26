use std::sync::atomic::{AtomicU8, Ordering};

static VERBOSITY: AtomicU8 = AtomicU8::new(0);

pub fn set(level: u8) {
    VERBOSITY.store(level, Ordering::Relaxed);
}

pub fn level() -> u8 {
    VERBOSITY.load(Ordering::Relaxed)
}

pub fn is_verbose() -> bool {
    level() >= 1
}

#[allow(dead_code)] // Available for -vv verbosity checks
pub fn is_debug() -> bool {
    level() >= 2
}

#[allow(dead_code)] // Available for -vvv verbosity checks
pub fn is_trace() -> bool {
    level() >= 3
}
