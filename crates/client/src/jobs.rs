//! One job in flight per worker. The frame thread submits and polls.
//!
//! It never waits. Terrain generation, chunk remesh, and the bounce rays
//! inside that remesh run here, on a snapshot, so the world stays on the
//! frame thread.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

pub struct Lane<T, U> {
    tx: Sender<Option<T>>,
    rx: Receiver<U>,
    busy: bool,
}

impl<T, U> Lane<T, U>
where
    T: Send + 'static,
    U: Send + 'static,
{
    pub fn new(name: &str, work: impl Fn(T) -> U + Send + 'static) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<Option<T>>();
        let (out_tx, out_rx) = mpsc::channel::<U>();
        thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                while let Ok(Some(job)) = job_rx.recv() {
                    if out_tx.send(work(job)).is_err() {
                        break;
                    }
                }
            })
            .expect("worker thread");
        Self {
            tx: job_tx,
            rx: out_rx,
            busy: false,
        }
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Queue `job` when this lane is idle. Returns false when one is running.
    pub fn submit(&mut self, job: T) -> bool {
        if self.busy {
            return false;
        }
        if self.tx.send(Some(job)).is_err() {
            return false;
        }
        self.busy = true;
        true
    }

    /// A finished job, if the worker has one. An empty lane stays empty.
    pub fn poll(&mut self) -> Option<U> {
        if !self.busy {
            return None;
        }
        match self.rx.try_recv() {
            Ok(value) => {
                self.busy = false;
                Some(value)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                None
            }
        }
    }
}

impl<T, U> Drop for Lane<T, U> {
    fn drop(&mut self) {
        let _ = self.tx.send(None);
    }
}

/// A few lanes that share one kind of job. Submit picks an idle lane.
pub struct Pool<T, U> {
    lanes: Vec<Lane<T, U>>,
}

impl<T, U> Pool<T, U>
where
    T: Send + 'static,
    U: Send + 'static,
{
    pub fn new(count: usize, name: &str, work: impl Fn(T) -> U + Send + Sync + 'static) -> Self {
        let work = std::sync::Arc::new(work);
        let lanes = (0..count.max(1))
            .map(|index| {
                let work = std::sync::Arc::clone(&work);
                Lane::new(&format!("{name}-{index}"), move |job| work(job))
            })
            .collect();
        Self { lanes }
    }

    pub fn has_idle(&self) -> bool {
        self.lanes.iter().any(|lane| !lane.busy())
    }

    pub fn submit(&mut self, job: T) -> bool {
        let Some(lane) = self.lanes.iter_mut().find(|lane| !lane.busy()) else {
            return false;
        };
        lane.submit(job)
    }

    pub fn poll(&mut self) -> Option<U> {
        for lane in &mut self.lanes {
            if let Some(value) = lane.poll() {
                return Some(value);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn submit_returns_before_the_worker_finishes() {
        let mut lane = Lane::new("test-worker", |n: u32| {
            thread::sleep(Duration::from_millis(40));
            n + 1
        });
        let started = Instant::now();
        assert!(lane.submit(4));
        assert!(
            started.elapsed() < Duration::from_millis(20),
            "submit waited for the worker"
        );
        assert!(lane.busy());
        assert!(lane.poll().is_none());
        let mut got = None;
        for _ in 0..40 {
            if let Some(value) = lane.poll() {
                got = Some(value);
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, Some(5));
        assert!(!lane.busy());
    }
}
