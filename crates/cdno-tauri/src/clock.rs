//! Day-change detection (plan §3.7): a 30s ticker thread that emits
//! `clock:day-changed` when the local calendar date rolls over —
//! laptops sleeping past midnight, timezone changes. The frontend
//! reacts by invalidating everything date-dependent.

use std::time::Duration;

use chrono::Local;
use tauri::{AppHandle, Emitter};

use crate::events::CLOCK_DAY_CHANGED;

const TICK: Duration = Duration::from_secs(30);

/// Run forever; spawn via `std::thread::spawn`.
pub fn run(app: AppHandle) {
    let mut today = Local::now().date_naive();
    loop {
        std::thread::sleep(TICK);
        let now = Local::now().date_naive();
        if now != today {
            today = now;
            let _ = app.emit(CLOCK_DAY_CHANGED, now.format("%Y-%m-%d").to_string());
        }
    }
}
