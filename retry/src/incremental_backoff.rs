use std::time::Duration;

use async_trait::async_trait;
use atomic::{Atomic, Ordering};

pub struct IncrementalInterval {
  wait_for: Atomic<Duration>,
  increment_by: Duration,
}

impl IncrementalInterval {
  pub fn new(wait_for: Duration, increment_by: Duration) -> Self {
    Self {
      wait_for: Atomic::new(wait_for),
      increment_by,
    }
  }
}

#[async_trait]
impl crate::Backoff for IncrementalInterval {
  async fn wait(&self, _retry: u32) {
    let wait_for = self.wait_for.load(Ordering::Acquire);

    tokio::time::sleep(wait_for).await;

    self
      .wait_for
      .store(wait_for + self.increment_by, Ordering::Release);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Retry;
  use std::{cell::Cell, rc::Rc, time::Instant};

  #[tokio::test]
  async fn smoke() {
    // Given
    let start = Instant::now();
    let tries = Rc::new(Cell::new(0));

    // When
    let result: Result<i32, &str> = Retry::new()
      .retries(3)
      .backoff(IncrementalInterval::new(
        Duration::from_secs(1),
        Duration::from_secs(1),
      ))
      .exec(|| async {
        tries.set(tries.get() + 1);

        if tries.get() == 3 {
          Ok(1)
        } else {
          Err("oops")
        }
      })
      .await;

    // Then
    assert_eq!(Ok(1), result);

    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_secs(2) && elapsed <= Duration::from_secs(4));
  }
}
