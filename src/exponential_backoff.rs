use std::time::Duration;

use async_trait::async_trait;

struct ExponentialBackoff {
  pub max: u32,
  pub start: u32,
}

#[async_trait]
impl crate::retry::Backoff for ExponentialBackoff {
  async fn wait(&mut self, retry: usize) {
    let duration = std::cmp::min(self.max, self.start * 2_u32.pow(retry as u32));
    tokio::time::sleep(Duration::from_secs(duration as u64)).await;
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
      .backoff(ExponentialBackoff { start: 1, max: 12 })
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

    assert!(elapsed >= Duration::from_secs(5) && elapsed <= Duration::from_secs(7));
  }
}
