use std::time::Duration;

use async_trait::async_trait;

struct ExponentialBackoff {
  pub exponent: u32,
  pub wait_for: u32,
}

impl ExponentialBackoff {
  /// The backoff time will be start^2.
  pub fn binary(wait_for: u32) -> Self {
    assert!(wait_for > 1, "initial waiting time must be greater than 1");
    Self {
      wait_for,
      exponent: 2,
    }
  }
}

#[async_trait]
impl crate::retry::Backoff for ExponentialBackoff {
  async fn wait(&mut self, _retry: usize) {
    tokio::time::sleep(Duration::from_secs(self.wait_for as u64)).await;
    self.wait_for = self.wait_for.pow(self.exponent);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::retry::Retry;
  use std::time::Instant;

  #[test]
  #[should_panic(expected = "initial waiting time must be greater than 1")]
  fn binary_initial_wait_time_must_be_greater_than_1() {
    let _ = ExponentialBackoff::binary(0);
  }

  #[tokio::test]
  async fn smoke() {
    let start = Instant::now();
    let mut tries = 0;

    let result: Result<i32, &str> = Retry::new()
      .retries(3)
      .backoff(ExponentialBackoff::binary(2))
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
