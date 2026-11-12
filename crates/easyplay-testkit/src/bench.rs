//! A minimal benchmark harness for the budgets of PERFORMANCE §1 (task PERF-001).
//!
//! Each bench target (`crates/*/benches/*.rs`, `harness = false`) calls [`Bench::run`] per
//! row. A run warms up, times `iterations` calls, prints one line with the median, p99 and
//! budget, and appends a JSON line to `$EASYPLAY_BENCH_OUT` when set. The regression gate
//! (`scripts/bench-gate.sh`) compares those lines with the budgets and with a recorded
//! baseline (25 % tolerance, TESTING_STRATEGY §11).
//!
//! Budgets are only meaningful in an optimised build: run with `cargo bench`. Under
//! `cargo test --benches` (debug), every row runs a few iterations as a smoke test and the
//! budgets are reported but never enforced.
//!
//! `EASYPLAY_BENCH_FILTER=<substring>` runs only matching rows.

use std::fmt::Write as _;
use std::hint::black_box;
use std::io::Write as _;
use std::time::Duration;

/// Which statistic the budget applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    /// The median.
    Median,
    /// The 99th percentile.
    P99,
}

/// Timings of one row.
#[derive(Clone, Debug)]
pub struct Report {
    /// Row name (the "Measured by" column).
    pub name: String,
    /// Iterations timed.
    pub iterations: usize,
    /// Median.
    pub median: Duration,
    /// 99th percentile.
    pub p99: Duration,
    /// Budget.
    pub budget: Duration,
    /// Statistic the budget applies to.
    pub stat: Stat,
}

impl Report {
    /// The measured value the budget applies to.
    pub fn measured(&self) -> Duration {
        match self.stat {
            Stat::Median => self.median,
            Stat::P99 => self.p99,
        }
    }

    /// Whether the budget is met.
    pub fn within_budget(&self) -> bool {
        self.measured() <= self.budget
    }

    /// One JSON object (no external serialiser needed).
    pub fn json(&self) -> String {
        let mut s = String::new();
        let _ = write!(
            s,
            "{{\"name\":\"{}\",\"iterations\":{},\"median_us\":{:.1},\"p99_us\":{:.1},\"budget_us\":{:.1},\"stat\":\"{}\",\"optimized\":{}}}",
            self.name,
            self.iterations,
            micros(self.median),
            micros(self.p99),
            micros(self.budget),
            match self.stat {
                Stat::Median => "median",
                Stat::P99 => "p99",
            },
            optimized(),
        );
        s
    }
}

fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

/// Whether this is an optimised build (budgets apply).
pub fn optimized() -> bool {
    !cfg!(debug_assertions)
}

/// One row of PERFORMANCE §1.
#[derive(Clone, Debug)]
pub struct Bench {
    name: String,
    budget: Duration,
    stat: Stat,
    iterations: usize,
    warmup: usize,
}

impl Bench {
    /// A row whose median must stay within `budget`.
    pub fn new(name: impl Into<String>, budget: Duration) -> Self {
        Self {
            name: name.into(),
            budget,
            stat: Stat::Median,
            iterations: 100,
            warmup: 5,
        }
    }

    /// Apply the budget to the 99th percentile instead.
    pub fn p99(mut self) -> Self {
        self.stat = Stat::P99;
        self
    }

    /// Timed iterations (default 100; a debug build caps them at 3).
    pub fn iterations(mut self, n: usize) -> Self {
        self.iterations = n.max(1);
        self
    }

    /// Whether `EASYPLAY_BENCH_FILTER` selects this row.
    pub fn selected(&self) -> bool {
        std::env::var("EASYPLAY_BENCH_FILTER").map_or(true, |f| self.name.contains(&f))
    }

    /// Time `f` (its result is kept alive through [`black_box`]).
    pub fn run<R>(&self, mut f: impl FnMut() -> R) -> Option<Report> {
        self.run_with(|| (), |()| f())
    }

    /// Time `f` on a fresh input from `setup` per iteration (setup is not timed).
    #[allow(clippy::disallowed_methods)] // Measuring wall time is the point of a bench.
    pub fn run_with<I, R>(
        &self,
        mut setup: impl FnMut() -> I,
        mut f: impl FnMut(I) -> R,
    ) -> Option<Report> {
        if !self.selected() {
            return None;
        }
        let (warmup, iterations) = if optimized() {
            (self.warmup, self.iterations)
        } else {
            (1, self.iterations.min(3))
        };
        for _ in 0..warmup {
            let input = setup();
            black_box(f(black_box(input)));
        }
        let mut times = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let input = setup();
            let t = std::time::Instant::now();
            black_box(f(black_box(input)));
            times.push(t.elapsed());
        }
        Some(self.report(times))
    }

    /// A report over externally measured samples (frame times from a harness, say).
    pub fn report(&self, mut times: Vec<Duration>) -> Report {
        times.sort_unstable();
        let at = |q: f64| -> Duration {
            let i = ((times.len() as f64 - 1.0) * q).round() as usize;
            times.get(i).copied().unwrap_or_default()
        };
        let r = Report {
            name: self.name.clone(),
            iterations: times.len(),
            median: at(0.5),
            p99: at(0.99),
            budget: self.budget,
            stat: self.stat,
        };
        emit(&r);
        r
    }
}

fn emit(r: &Report) {
    let verdict = if !optimized() {
        "smoke (debug build)"
    } else if r.within_budget() {
        "ok"
    } else {
        "OVER BUDGET"
    };
    println!(
        "{:<28} median {:>10.1} µs  p99 {:>10.1} µs  budget {:>9.1} µs ({})  {verdict}",
        r.name,
        micros(r.median),
        micros(r.p99),
        micros(r.budget),
        match r.stat {
            Stat::Median => "median",
            Stat::P99 => "p99",
        },
    );
    if let Some(path) = std::env::var_os("EASYPLAY_BENCH_OUT")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{}", r.json());
    }
}

/// Exit status for a bench binary: non-zero when `EASYPLAY_BENCH_ENFORCE=1`, the build is
/// optimised and a row is over budget.
pub fn finish(reports: &[Option<Report>]) {
    let over: Vec<&Report> = reports
        .iter()
        .flatten()
        .filter(|r| !r.within_budget())
        .collect();
    let enforce = std::env::var_os("EASYPLAY_BENCH_ENFORCE").is_some_and(|v| v == "1");
    if enforce && optimized() && !over.is_empty() {
        for r in over {
            eprintln!("over budget: {}", r.name);
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_statistics() {
        let b = Bench::new("x", Duration::from_millis(5));
        let times: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let r = b.report(times);
        assert_eq!(r.median, Duration::from_millis(51));
        assert_eq!(r.p99, Duration::from_millis(99));
        assert!(!r.within_budget());
        assert!(r.json().contains("\"name\":\"x\""));
        let r = b.clone().p99().report(vec![Duration::from_millis(1); 10]);
        assert!(r.within_budget());
    }
}
