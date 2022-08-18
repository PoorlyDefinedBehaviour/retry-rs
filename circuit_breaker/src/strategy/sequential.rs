use std::{
  fmt::Display,
  sync::atomic::{AtomicU32, Ordering},
  time::{Duration, Instant},
};

use async_trait::async_trait;

use crate::CircuitBreakerStrategy;
use atomic::Atomic;

pub struct ConsecutiveFailures {
  max_error_count: u32,
  error_count: AtomicU32,
  opened_at: Atomic<Option<Instant>>,
  backoff: Duration,
}

impl Default for ConsecutiveFailures {
  fn default() -> Self {
    Self::new()
  }
}

impl ConsecutiveFailures {
  pub fn new() -> Self {
    Self {
      error_count: AtomicU32::new(0),
      max_error_count: 10,
      opened_at: Atomic::new(None),
      backoff: Duration::from_secs(3),
    }
  }

  pub fn max_error_count(mut self, n: u32) -> Self {
    self.max_error_count = n;
    self
  }

  pub fn backoff(mut self, duration: Duration) -> Self {
    self.backoff = duration;
    self
  }
}

#[async_trait]
impl CircuitBreakerStrategy for ConsecutiveFailures {
  async fn should_open(&self) -> bool {
    let should_open = dbg!(self.error_count.load(Ordering::SeqCst)) >= dbg!(self.max_error_count);

    if should_open {
      self.opened_at.store(Some(Instant::now()), Ordering::SeqCst);
    }

    should_open
  }

  async fn should_close(&self) -> bool {
    let should_close = self.error_count.load(Ordering::SeqCst) == 0;

    if should_close {
      self.opened_at.store(None, Ordering::SeqCst);
    }

    should_close
  }

  async fn should_half_open(&self) -> bool {
    match self.opened_at.load(Ordering::SeqCst) {
      None => false,
      Some(instant) => instant.elapsed() >= self.backoff,
    }
  }

  async fn on_error(&self, _attempt: u16, _err: &(dyn Display + Send + Sync)) {
    self.error_count.fetch_add(1, Ordering::SeqCst);
  }

  async fn success(&self, _attempt: u16) {
    loop {
      if self
        .error_count
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
          if value > 1 {
            Some(value - 1)
          } else {
            Some(0)
          }
        })
        .is_ok()
      {
        break;
      }
    }
  }
}

#[cfg(test)]
mod tests {

  use std::ops::Sub;

  use super::*;

  #[tokio::test]
  async fn on_error_increases_error_count() {
    let s = ConsecutiveFailures::new();
    s.on_error(1, &anyhow::anyhow!("error")).await;
    assert_eq!(1, s.error_count.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn opens_when_max_error_count_is_reached() {
    let s = ConsecutiveFailures::new()
      .max_error_count(3)
      .backoff(Duration::from_secs(3));

    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;

    assert!(s.should_open().await);
  }

  #[tokio::test]
  async fn closes_when_error_count_is_0() {
    let s = ConsecutiveFailures::new()
      .max_error_count(3)
      .backoff(Duration::from_secs(3));

    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;

    assert!(s.should_open().await);

    s.success(1).await;
    s.success(1).await;
    s.success(1).await;

    assert!(s.should_close().await);
    assert!(!s.should_open().await);
  }

  #[tokio::test]
  async fn goes_to_half_open_after_the_backoff_duration() {
    let s = ConsecutiveFailures::new()
      .max_error_count(3)
      .backoff(Duration::from_secs(3));

    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;
    s.on_error(1, &anyhow::anyhow!("error")).await;

    assert!(!s.should_half_open().await);
    assert!(s.should_open().await);

    s.opened_at
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |instant| {
        Some(Some(instant.unwrap().sub(Duration::from_secs(5))))
      })
      .unwrap();

    // 5 seconds have passed and the backoff is 3 seconds
    assert!(s.should_half_open().await);
  }
}
