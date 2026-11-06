//! Heartbeat and progress lines for a memory-shape run.
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct Live {
    pub(super) phase: &'static str,
    pub(super) index: usize,
    pub(super) total: usize,
    pub(super) label: String,
    pub(super) statement_started: Instant,
    pub(super) phase_started: Instant,
    pub(super) last_print: Instant,
    pub(super) active: bool,
}

impl Default for Live {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            phase: "probe",
            index: 0,
            total: 0,
            label: String::new(),
            statement_started: now,
            phase_started: now,
            last_print: now,
            active: false,
        }
    }
}

pub(super) fn progress(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub(super) fn format_elapsed(elapsed: Duration) -> String {
    let whole = elapsed.as_secs();
    if whole < 60 {
        format!("{}.{:01}s", whole, elapsed.subsec_millis() / 100)
    } else if whole < 3600 {
        format!("{}m {:02}s", whole / 60, whole % 60)
    } else {
        format!("{}h {:02}m", whole / 3600, (whole % 3600) / 60)
    }
}

pub(super) fn abbreviate(statement: &str) -> String {
    let compact = statement.split_whitespace().collect::<Vec<_>>().join(" ");
    const WIDTH: usize = 52;
    if compact.chars().count() <= WIDTH {
        return compact;
    }
    let truncated: String = compact.chars().take(WIDTH - 1).collect();
    format!("{truncated}…")
}

pub(super) fn mark(live: &Mutex<Live>, phase: &'static str, label: &str) {
    if let Ok(mut guard) = live.lock() {
        guard.phase = phase;
        guard.index = 0;
        guard.total = 0;
        guard.label = label.to_string();
        guard.statement_started = Instant::now();
        guard.active = true;
    }
}

pub(super) fn clear_active(live: &Mutex<Live>) {
    if let Ok(mut guard) = live.lock() {
        guard.active = false;
        guard.last_print = Instant::now();
    }
}

pub(super) struct Heartbeat {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Heartbeat {
    pub(super) fn start(live: Arc<Mutex<Live>>) -> Self {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let handle = spawn_heartbeat(live, Arc::clone(&stop));
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        let (lock, wake) = &*self.stop;
        if let Ok(mut stop) = lock.lock() {
            *stop = true;
            wake.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_heartbeat(
    live: Arc<Mutex<Live>>,
    stop: Arc<(Mutex<bool>, Condvar)>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        {
            let (lock, wake) = &*stop;
            let Ok(flag) = lock.lock() else {
                break;
            };
            if *flag {
                break;
            }
            let (flag, _) = match wake.wait_timeout(flag, Duration::from_secs(5)) {
                Ok(waited) => waited,
                Err(err) => err.into_inner(),
            };
            if *flag {
                break;
            }
        }
        let line = {
            let Ok(mut guard) = live.lock() else {
                break;
            };
            let running = guard.statement_started.elapsed();
            let quiet = guard.last_print.elapsed();
            if !(guard.active
                && running >= Duration::from_secs(15)
                && quiet >= Duration::from_secs(15))
            {
                continue;
            }
            let line = if guard.total == 0 && guard.index == 0 {
                format!(
                    "      {:<5} {:<52} still running {}",
                    guard.phase,
                    guard.label,
                    format_elapsed(running)
                )
            } else if guard.total == 0 {
                format!(
                    "      {:<5} {:>4}      {:<52} still running {}",
                    guard.phase,
                    guard.index,
                    guard.label,
                    format_elapsed(running)
                )
            } else {
                format!(
                    "      {:<5} {:>4}/{:<4} {:<52} still running {}",
                    guard.phase,
                    guard.index,
                    guard.total,
                    guard.label,
                    format_elapsed(running)
                )
            };
            guard.last_print = Instant::now();
            line
        };
        progress(&line);
    })
}
