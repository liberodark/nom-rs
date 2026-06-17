pub fn now() -> f64 {
    static_clock::elapsed_seconds()
}

mod static_clock {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    pub fn elapsed_seconds() -> f64 {
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_secs_f64()
    }
}

/// `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn utc_now_string() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Local-time clock string for the bottom-of-the-screen status: `HH:MM:SS`.
pub fn local_clock_string() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
