use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;

pub struct FullJitterExponentialBackoff {
  pub max: u32,
  pub start: u32,
}

impl FullJitterExponentialBackoff {
  pub fn recommended() -> Self {
    Self {
      // TODO: find good values
      max: 1,
      start: 12,
    }
  }
}

#[async_trait]
impl crate::Backoff for FullJitterExponentialBackoff {
  async fn wait(&self, retry: u32) {
    let duration = rand::thread_rng()
      .gen_range(0..=std::cmp::min(self.max, self.start * 2_u32.pow(retry as u32)));

    tokio::time::sleep(Duration::from_secs(duration as u64)).await;
  }
}

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use super::*;
  use crate::Retry;

  #[tokio::test]
  async fn smoke() {
    // Given
    let tries = Rc::new(Cell::new(0));

    // When
    let result: Result<i32, &str> = Retry::new()
      .retries(3)
      .backoff(FullJitterExponentialBackoff::recommended())
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
  }
}
