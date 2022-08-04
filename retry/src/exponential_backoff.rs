//! Wait for an expontially increasing amount of time between retries.

use std::time::Duration;

use async_trait::async_trait;
use tracing::info;

/// ```
/// # use retry::{Retry, ExponentialBackoff};
/// # async fn get(_: &str) -> Result<i32, String> {
/// #   Ok(1)
/// # }
/// # tokio_test::block_on(async {
///let result = Retry::new()
///   .retries(5)
///   .backoff(ExponentialBackoff::recommended())
///   .exec(|| async {
///     let response: Result<i32, String> = get("https://example.com/1").await;
///     response
///   })
///   .await;
///
///assert_eq!(Ok(1), result);
/// # })
/// ```
pub struct ExponentialBackoff {
  pub start: u32,
  pub max: u32,
}

impl ExponentialBackoff {
  pub fn recommended() -> Self {
    // TODO: find nice values
    Self { start: 1, max: 12 }
  }
}

#[async_trait]
impl crate::Backoff for ExponentialBackoff {
  #[tracing::instrument(skip_all, fields(retry = %retry))]
  async fn wait(&self, retry: u32) {
    let duration = std::cmp::min(self.max, self.start * 2_u32.pow(retry as u32));
    info!("backing off. seconds={}", duration);
    tokio::time::sleep(Duration::from_secs(duration as u64)).await;
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
      .backoff(ExponentialBackoff { start: 1, max: 12 })
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

    assert!(elapsed >= Duration::from_secs(5) && elapsed <= Duration::from_secs(7));
  }
}
