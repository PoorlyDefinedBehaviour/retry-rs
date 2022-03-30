use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;

struct FullJitterExponentialBackoff {
  pub max: u32,
  pub start: u32,
}

#[async_trait]
impl crate::retry::Backoff for FullJitterExponentialBackoff {
  async fn wait(&mut self, retry: usize) {
    let duration = rand::thread_rng()
      .gen_range(0..=std::cmp::min(self.max, self.start * 2_u32.pow(retry as u32)));

    tokio::time::sleep(Duration::from_secs(duration as u64)).await;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::retry::Retry;

  #[tokio::test]
  async fn smoke() {
    let mut tries = 0;

    let result: Result<i32, &str> = Retry::new()
      .retries(3)
      .backoff(FullJitterExponentialBackoff { start: 1, max: 12 })
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
  }
}
