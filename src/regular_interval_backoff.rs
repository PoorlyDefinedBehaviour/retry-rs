use std::time::Duration;

use async_trait::async_trait;

struct RegularIntervalBackoff {
  wait_for: Duration,
}

#[async_trait]
impl crate::retry::Backoff for RegularIntervalBackoff {
  async fn wait(&mut self, _retry: usize) {
    tokio::time::sleep(self.wait_for).await
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::retry::Retry;
  use std::time::Instant;

  #[tokio::test]
  async fn smoke() {
    let start = Instant::now();
    let mut tries = 0;

    let result: Result<i32, &str> = Retry::new()
      .retries(3)
      .backoff(RegularIntervalBackoff {
        wait_for: Duration::from_millis(50),
      })
      .exec(|| {
        tries += 1;

        if tries == 3 {
          Ok(1)
        } else {
          Err("oops")
        }
      })
      .await;

    assert_eq!(Ok(1), result);

    let elapsed = start.elapsed();

    assert!(elapsed >= Duration::from_millis(90) && elapsed <= Duration::from_millis(110));
  }
}
