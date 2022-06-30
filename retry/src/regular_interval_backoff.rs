use std::time::Duration;

use async_trait::async_trait;

pub struct RegularIntervalBackoff {
  wait_for: Duration,
}

#[async_trait]
impl crate::Backoff for RegularIntervalBackoff {
  async fn wait(&self, _retry: u32) {
    tokio::time::sleep(self.wait_for).await
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
      .backoff(RegularIntervalBackoff {
        wait_for: Duration::from_millis(50),
      })
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

    assert!(elapsed >= Duration::from_millis(90) && elapsed <= Duration::from_millis(110));
  }
}
