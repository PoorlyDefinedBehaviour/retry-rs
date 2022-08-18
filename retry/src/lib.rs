//! This module contains the controller that handles retries. The number of retries and the backoff strategy are configurable.

pub mod exponential_backoff;
pub mod full_jitter_exponential_backoff;
pub mod incremental_backoff;
pub mod regular_interval_backoff;

pub use exponential_backoff::ExponentialBackoff;
pub use full_jitter_exponential_backoff::FullJitterExponentialBackoff;
pub use incremental_backoff::IncrementalInterval;
pub use regular_interval_backoff::RegularIntervalBackoff;

use async_trait::async_trait;
use std::{fmt::Debug, future::Future, sync::Arc};

use tracing::warn;

/// ## Decide if a retry should happen
///
/// Retries happen when the max number of retries has not been reached and the closure returns an Error
/// but you can abort if you find an unrecoverable error.
///
/// Will retry 3 times only if the service returns 500.
///
///```
/// # use retry::{Retry, RetryResult};
/// # use retry::exponential_backoff::ExponentialBackoff;
/// # async fn post(_: &str) -> Result<i32, String> {
/// #   Ok(1)
/// # }
/// # tokio_test::block_on(async {
///let result = Retry::new().backoff(ExponentialBackoff::recommended()).retries(3).exec(|| async {
///   let response_status = match post("https://example.com/1").await {
///     Err(err) => return RetryResult::Retry(err),
///     Ok(response_status) => response_status
///   };
///
///   if response_status >= 400 && response_status <= 500 {
///     RetryResult::DontRetry(String::from("won't retry this one"))
///   } else if response_status >= 500 {
///     RetryResult::Retry(String::from("got 500 but we'll retry"))
///   } else {
///     RetryResult::Ok(())
///   }
///})
///.await;
/// # })
///```
///
/// ## Simple retry without backoff
///
/// Will retry 3 times in a row without waiting between retries if an Err is returned from the closure.
///
///```
/// # use retry::Retry;
/// # async fn get(_: &str) -> Result<i32, String> {
/// #   Ok(1)
/// # }
/// # tokio_test::block_on(async {
///let result = Retry::new().retries(3).exec(|| async {
///  let response: Result<i32, String> = get("https://example.com/1").await;
///  response
///})
///.await;
///
///assert_eq!(Ok(1), result);
/// # })
///```
pub struct Retry {
  retries: u32,
  backoff: Option<Arc<dyn Backoff>>,
}

pub struct NoOpBackoff;

#[async_trait]
impl Backoff for NoOpBackoff {
  async fn wait(&self, _retry: u32) {}
}

#[async_trait]
pub trait Backoff {
  async fn wait(&self, retry: u32);
}

impl Retry {
  pub fn new() -> Self {
    Self {
      retries: 1,
      backoff: None,
    }
  }
}

impl Default for Retry {
  fn default() -> Self {
    Self::new()
  }
}

pub enum RetryResult<T, E> {
  Retry(E),
  DontRetry(E),
  Ok(T),
}

impl<T, E> From<Result<T, E>> for RetryResult<T, E> {
  fn from(input: Result<T, E>) -> RetryResult<T, E> {
    match input {
      Err(err) => RetryResult::Retry(err),
      Ok(v) => RetryResult::Ok(v),
    }
  }
}

impl Retry {
  pub fn retries(&mut self, n: u32) -> &mut Self {
    assert!(n > 0, "retries must be greater than 0");
    self.retries = n;
    self
  }

  pub fn backoff(&mut self, b: impl Backoff + 'static) -> &mut Self {
    self.backoff = Some(Arc::new(b));
    self
  }

  pub async fn exec<T, E, F, Value, Out>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> Out,
    Value: Into<RetryResult<T, E>>,
    Out: Future<Output = Value>,
    E: Debug,
  {
    let mut tries = 0;

    loop {
      match f().await.into() {
        RetryResult::Retry(err) => {
          tries += 1;

          // TODO: notify subscriber?
          warn!(retries = self.retries, ?tries, ?err);

          if tries >= self.retries {
            return Err(err);
          }

          if let Some(ref backoff) = self.backoff {
            backoff.wait(tries).await;
          }
        }
        RetryResult::DontRetry(err) => return Err(err),
        RetryResult::Ok(value) => return Ok(value),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use super::*;
  use proptest::prelude::*;
  use tokio::runtime::Runtime;

  #[test]
  #[should_panic(expected = "retries must be greater than 0")]
  fn retries_must_be_greater_than_0() {
    // Given
    // Then
    Retry::new()
      .backoff(ExponentialBackoff::recommended())
      .retries(0);
  }

  #[tokio::test]
  async fn retries_defaults_to_1() {
    // Given
    let tries = Rc::new(Cell::new(0));

    // When
    let result: Result<i32, &str> = Retry::new()
      .backoff(ExponentialBackoff::recommended())
      .exec(|| async {
        tries.set(tries.get() + 1);
        Err("oops")
      })
      .await;

    // Then
    assert_eq!(Err("oops"), result);
    assert_eq!(1, tries.get());
  }

  proptest! {
    #[test]
    fn function_that_returns_result_error_is_returned_if_task_never_succeeds(num_retries in 1..=1000_u32) {
      Runtime::new().unwrap().block_on(async {
        // Given
        let tries = Rc::new(Cell::new(0));

        let result: Result<i32, &str> = Retry::new().backoff(NoOpBackoff).retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          // When
          Err("nope")
        })
        .await;

        // Then
        assert_eq!(Err("nope"), result);
        assert_eq!(tries.get(), num_retries);
      });
    }

    #[test]
    fn function_that_returns_result_exec_succeeds_on_nth_retry(num_retries in 1..=1000_u32) {
      Runtime::new().unwrap().block_on(async {
        // Given
        let tries = Rc::new(Cell::new(0));

        let result = Retry::new().backoff(NoOpBackoff).retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          // When
          if tries.get() == num_retries {
            Ok(1)
          } else {
            Err("oops")
          }
        }).await;

        // Then
        assert_eq!(Ok(1), result);
        assert_eq!(num_retries, tries.get());
      });
    }

    #[test]
    fn function_that_returns_retry_result_error_is_returned_if_task_never_succeeds(num_retries in 1..=1000_u32) {
      Runtime::new().unwrap().block_on(async {
        // Given
        let tries = Rc::new(Cell::new(0));

        let result: Result<i32, &str> = Retry::new().backoff(NoOpBackoff).retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          // When
          RetryResult::Retry("nope")
        })
        .await;

        // Then
        assert_eq!(Err("nope"), result);
        assert_eq!(tries.get(), num_retries);
      });
    }

    #[test]
    fn function_that_returns_retry_result_error_is_returned_if_task_cannot_recover(num_retries in 1..=1000_u32) {
      Runtime::new().unwrap().block_on(async {
        // Given
        let tries = Rc::new(Cell::new(0));

        let result: Result<i32, &str> = Retry::new().backoff(NoOpBackoff).retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          // When
          RetryResult::DontRetry("can't recover from this one")
        })
        .await;

        // Then
        assert_eq!(Err("can't recover from this one"), result);
        assert_eq!(tries.get(), 1);
      });
    }


    #[test]
    fn function_that_returns_retry_result_exec_succeeds_on_nth_retry(num_retries in 1..=1000_u32) {
      Runtime::new().unwrap().block_on(async {
        // Given
        let tries = Rc::new(Cell::new(0));

        let result = Retry::new().backoff(NoOpBackoff).retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          // When
          if tries.get() == num_retries {
            RetryResult::Ok(1)
          } else {
            RetryResult::Retry(String::from("something went wrong"))
          }
        }).await;

        // Then
        assert_eq!(Ok(1), result);
        assert_eq!(num_retries, tries.get());
      });
    }

  }
}
