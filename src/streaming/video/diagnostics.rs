use std::io::{BufWriter, Write};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
static STARTED: OnceLock<()> = OnceLock::new();
pub(crate) static CATCH_UP: AtomicU64 = AtomicU64::new(0);
pub(crate) static DAMAGE: AtomicU64 = AtomicU64::new(0);
pub(crate) static QUEUE_FULL: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Stage {
    count: AtomicU64,
    last_us: AtomicU64,
    max_gap_us: AtomicU64,
}
impl Stage {
    const fn new() -> Self {
        Self { count: AtomicU64::new(0), last_us: AtomicU64::new(0), max_gap_us: AtomicU64::new(0) }
    }
    pub(crate) fn mark(&self) {
        let now = START.get_or_init(Instant::now).elapsed().as_micros() as u64 + 1;
        let previous = self.last_us.swap(now, Ordering::Relaxed);
        if previous != 0 {
            self.max_gap_us.fetch_max(now.saturating_sub(previous), Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
    }
    fn sample(&self, now: u64) -> (u64, u64, u64) {
        let last = self.last_us.load(Ordering::Relaxed);
        (self.count.load(Ordering::Relaxed),
         self.max_gap_us.swap(0, Ordering::Relaxed),
         if last == 0 { 0 } else { now.saturating_sub(last) })
    }
}
pub(crate) static ASSEMBLED: Stage = Stage::new();
pub(crate) static DECODED: Stage = Stage::new();
pub(crate) static PRESENTED: Stage = Stage::new();
pub(crate) static RENDER_LOOP: Stage = Stage::new();

pub(crate) fn start() {
    STARTED.get_or_init(|| {
        START.get_or_init(Instant::now);
        if let Err(error) = std::thread::Builder::new()
            .name("green-vita-diagnostics".to_owned())
            .spawn(|| {
                if let Err(error) = record() {
                    eprintln!("Microcortes diagnostic log failed: {error}");
                }
            })
        {
            eprintln!("Could not start microcortes diagnostics: {error}");
        }
    });
}

fn record() -> std::io::Result<()> {
    // Wait for actual video, so sign-in time does not consume the capture window.
    while PRESENTED.count.load(Ordering::Relaxed) == 0 {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::create_dir_all("ux0:data/xcloud-rust")?;
    let file = std::fs::File::create("ux0:data/xcloud-rust/microcortes-diagnostico.csv")?;
    let mut out = BufWriter::new(file);
    writeln!(out, "# GreenVita diagnostic catch-up test 5; counts cumulative; gaps/ages in microseconds; pauses and menu transitions also create gaps")?;
    writeln!(out, "elapsed_us,assembled,assembly_gap_max_us,assembly_idle_us,decoded,decode_gap_max_us,decode_idle_us,presented,present_gap_max_us,present_idle_us,render_loops,render_gap_max_us,render_idle_us,damage_events,queue_full,resyncs,resets,last_decode_us,last_pipeline_age_us,catch_up_decoded")?;
    for row in 0..6000 {
        std::thread::sleep(Duration::from_millis(100));
        let now = START.get().expect("diagnostic clock").elapsed().as_micros() as u64 + 1;
        let a = ASSEMBLED.sample(now);
        let d = DECODED.sample(now);
        let p = PRESENTED.sample(now);
        let r = RENDER_LOOP.sample(now);
        let m = &super::metrics::METRICS;
        writeln!(out, "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            now, a.0, a.1, a.2, d.0, d.1, d.2, p.0, p.1, p.2, r.0, r.1, r.2,
            DAMAGE.load(Ordering::Relaxed), QUEUE_FULL.load(Ordering::Relaxed),
            m.resyncs.load(Ordering::Relaxed), m.resets.load(Ordering::Relaxed),
            m.decode_us.load(Ordering::Relaxed), m.pipeline_age_us.load(Ordering::Relaxed), CATCH_UP.load(Ordering::Relaxed))?;
        if row % 10 == 9 { out.flush()?; }
    }
    out.flush()
}
