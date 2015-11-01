use std::io::{Write, Read, Result};
use std::time::Duration;
use std::sync::{Arc, RwLock};

use time;

fn now_as_float() -> f64 {
    let t = time::get_time();
    t.sec as f64 + t.nsec as f64 * 0.000000001
}

fn duration_as_float(dur: Duration) -> f64 {
    dur.as_secs() as f64 + dur.subsec_nanos() as f64 * 0.000000001
}

pub struct Stats {
    total: u64,
    rate: f64,
    last: f64,
    halftime: f64
}

impl Stats {
    pub fn new(halftime: Duration) -> Self {
        Stats{total: 0, rate: 0.0, last: now_as_float(), halftime: duration_as_float(halftime)}
    }

    pub fn update(&mut self, amount: usize) {
        self.total += amount as u64;
        let now = now_as_float();
        let alpha = 0.5f64.powf((now-self.last)/self.halftime);
        self.rate = self.rate * alpha + amount as f64 * (1.0f64 - alpha);
        self.last = now;
    }

    pub fn rate(&self) -> f64 {
        let now = now_as_float();
        let alpha = 0.5f64.powf((now-self.last)/self.halftime);
        self.rate * alpha
    }

    pub fn idle_time(&self) -> Duration {
        let diff = now_as_float() - self.last;
        Duration::new(diff.floor() as u64, ((diff - diff.floor())*1000000000.0) as u32)
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}


pub struct StatWriter<T> {
    inner: T,
    stats: Arc<RwLock<Stats>>
}

impl<T> StatWriter<T> {
    pub fn new(inner: T, halflife: Duration) -> Self {
        StatWriter{inner: inner, stats: Arc::new(RwLock::new(Stats::new(halflife)))}
    }

    pub fn stats(&self) -> Arc<RwLock<Stats>> {
        self.stats.clone()
    }
}

impl<T: Write> Write for StatWriter<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let bytes = try!(self.inner.write(buf));
        self.stats.write().expect("Lock poisoned").update(bytes);
        Ok(bytes)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}


pub struct StatReader<T> {
    inner: T,
    stats: Arc<RwLock<Stats>>
}

impl<T> StatReader<T> {
    pub fn new(inner: T, halflife: Duration) -> Self {
        StatReader{inner: inner, stats: Arc::new(RwLock::new(Stats::new(halflife)))}
    }

    pub fn stats(&self) -> Arc<RwLock<Stats>> {
        self.stats.clone()
    }
}

impl<T: Read> Read for StatReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let bytes = try!(self.inner.read(buf));
        self.stats.write().expect("Lock poisoned").update(bytes);
        Ok(bytes)
    }
}
