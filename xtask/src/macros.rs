macro_rules! windows_skip {
    () => {
        if cfg!(target_os = "windows") {
            eprintln!("Skip (unsupported on windows)");
            return Ok(());
        }
    };
}

pub(crate) use windows_skip;

macro_rules! trace {
    ($($arg:tt)*) => {{
        if $crate::is_verbose() {
            eprintln!($($arg)*);
        }
    }};
}

pub(crate) use trace;
