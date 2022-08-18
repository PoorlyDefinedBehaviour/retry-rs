use std::{
  fmt::Display,
  future::Future,
  sync::{
    atomic::{AtomicI8, Ordering},
    Arc,
  },
};
mod clock;
pub mod strategy;
use async_trait::async_trait;
use retry::{Backoff, RetryResult};
use strategy::sequential::ConsecutiveFailures;

const CLOSED: i8 = 0;
const OPEN: i8 = 1;
const HALF_OPEN: i8 = 2;

/// Contains information about the circuit breaker state.
#[derive(Debug)]
pub struct Context {
  /// Circuit breaker identifier. Can be anything.
  pub id: String,
}

/// Decides when the circuit breaker should transition to another state.
#[async_trait]
pub trait CircuitBreakerStrategy: Send + Sync {
  /// Called when the circuit transitions to the open state.
  async fn on_open(&self) {
    /* no-op */
  }

  /// Called when the circuit transitions to the half open state.
  async fn on_half_open(&self) {
    /* no-op */
  }

  /// Called when the circuit transitions to the closed state.
  async fn on_close(&self) {
    /* no-op */
  }

  /// Called when an execution succeeds.
  async fn success(&self, _attempt: u16) {
    /* no-op */
  }

  /// Called when an error happens.
  async fn on_error(&self, _attempt: u16, _err: &(dyn Display + Send + Sync)) {
    /* no-op */
  }

  /// The circuit closes if true is returned.
  async fn should_close(&self) -> bool;

  /// The circuit opens if true is returned.
  async fn should_open(&self) -> bool;

  /// The circuit half opens if true is returned.
  async fn should_half_open(&self) -> bool;
}

struct CircuitBreakerInner {
  /// Is the circuit breaker closed, open or half open?
  state: AtomicI8,
}

pub struct CircuitBreaker {
  /// The state that is shared between circuit breaker instances.
  inner: Arc<CircuitBreakerInner>,
  /// The number of retries before considering a call as failed.
  retries: u32,
  /// The backoff strategy.
  backoff: Option<Arc<dyn Backoff>>,
  /// The strategy used to decide when the circuit should transition to another state.
  strategy: Arc<dyn CircuitBreakerStrategy>,
}

impl CircuitBreaker {
  #[tracing::instrument(skip_all)]
  pub fn new() -> Self {
    Self {
      inner: Arc::new(CircuitBreakerInner {
        // Circuit starts closed.
        state: AtomicI8::new(CLOSED),
      }),
      /// Will try to execute the closure passed to exec only once by default.
      retries: 1,
      backoff: None,
      strategy: Arc::new(ConsecutiveFailures::new()),
    }
  }

  /// Sets the number of retries for the next call.
  pub fn with_retries(&self, n: u32) -> Self {
    assert!(n > 0, "retries must be greater than 0");
    Self {
      inner: Arc::clone(&self.inner),
      retries: n,
      backoff: self.backoff.clone(),
      strategy: self.strategy.clone(),
    }
  }

  /// Sets backoff strategy for the next call.
  pub fn with_backoff(&self, backoff: impl Backoff + Send + Sync + 'static) -> Self {
    Self {
      inner: Arc::clone(&self.inner),
      backoff: Some(Arc::new(backoff)),
      strategy: Arc::clone(&self.strategy),
      ..*self
    }
  }

  /// Returns true when the circuit is open.
  #[tracing::instrument(skip_all)]
  pub fn is_open(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == OPEN
  }

  /// Returns true when the circuit is half open.
  #[tracing::instrument(skip_all)]
  pub fn is_half_open(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == HALF_OPEN
  }

  /// Returns true when the circuit is closed.
  #[tracing::instrument(skip_all)]
  pub fn is_closed(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == CLOSED
  }

  /// Opens the circuit.
  #[tracing::instrument(skip_all)]
  pub fn open(&self) {
    self.inner.state.store(OPEN, Ordering::Release);
  }

  /// Transitions the circuit to half open.
  #[tracing::instrument(skip_all)]
  pub fn half_open(&self) {
    self.inner.state.store(HALF_OPEN, Ordering::Release);
  }

  /// Closes the circuit.
  #[tracing::instrument(skip_all)]
  pub fn close(&self) {
    self.inner.state.store(CLOSED, Ordering::Release);
  }
}

impl Default for CircuitBreaker {
  fn default() -> Self {
    Self::new()
  }
}

impl CircuitBreaker {
  #[tracing::instrument(skip_all)]
  pub async fn exec<F, T, E, Fut, Out>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> Fut,
    Fut: Future<Output = Out>,
    Out: Into<RetryResult<T, E>>,
  {
    let mut retry = 0;
    loop {
      match f().await.into() {
        RetryResult::Retry(err) => {
          if retry == self.retries {
            return Err(err);
          }

          if let Some(ref backoff) = self.backoff {
            backoff.wait(retry).await;
          }
        }
        RetryResult::DontRetry(err) => return Err(err),
        RetryResult::Ok(value) => return Ok(value),
      }
      retry += 1;
    }
  }
}

#[cfg(test)]
mod tests {
  use std::{cell::Cell, rc::Rc};

  use super::*;
  use async_trait::async_trait;
  use mockall::mock;
  use proptest::prelude::*;
  use tokio::runtime::Runtime;

  mock! {
      Backoff {}
      #[async_trait]
      impl retry::Backoff for Backoff {
        async fn wait(&self, retry: u32);
      }
  }

  #[test]
  #[should_panic(expected = "retries must be greater than 0")]
  fn retries_must_be_greater_than_0() {
    CircuitBreaker::new().with_retries(0);
  }

  #[test]
  fn cb_starts_closed() {
    let cb = CircuitBreaker::new();
    assert!(cb.is_closed());
  }

  #[test]
  fn cb_can_be_opened_manually() {
    let cb = CircuitBreaker::new();
    cb.open();
    assert!(cb.is_open());
  }

  #[test]
  fn cb_can_be_half_opened_manually() {
    let cb = CircuitBreaker::new();
    cb.half_open();
    assert!(cb.is_half_open());
  }

  #[test]
  fn cb_can_be_closed_manually() {
    let cb = CircuitBreaker::new();
    cb.open();
    assert!(cb.is_open());
    cb.close();
    assert!(cb.is_closed());
  }

  proptest! {
    #[test]
    fn can_retry_before_considering_a_call_as_failed(num_retries:u16) {
      prop_assume!(num_retries > 0);

      Runtime::new().unwrap().block_on(async {
        let cb = CircuitBreaker::new();

        let tries = Rc::new(Cell::new(0));

        let result = cb
          .with_retries(num_retries as u32)
          .exec(|| async {
            tries.set(tries.get() + 1);
            if tries.get() == num_retries {
              Ok(1)
            } else {
              Err(0)
            }
          })
          .await;

        assert_eq!(Ok(1), result);
        assert_eq!(num_retries, tries.get());
      });
    }

    #[test]
    fn add_backoff_between_retries(num_retries in 0..1000_u16) {
      prop_assume!(num_retries > 0);
      Runtime::new().unwrap().block_on(async {
        let cb = CircuitBreaker::new();

        let tries = Rc::new(Cell::new(0_u16));
        let times_backoff_got_called = Rc::new(Cell::new(0_u16));


        let tries_clone = Rc::clone(&tries);
        let times_backoff_got_called_clone = Rc::clone(&times_backoff_got_called);

        let mut backoff = MockBackoff::new();

        backoff
          .expect_wait()
          .times(num_retries as usize)
          .withf_st(move |retry_number| *retry_number == tries_clone.get() as u32-1)
          .returning_st(move |_| times_backoff_got_called_clone.set(times_backoff_got_called_clone.get()+1));

        let result = cb
          .with_retries(num_retries as u32)
          .with_backoff(backoff)
          .exec(|| async {
            let result = if tries.get() == num_retries  {
              Ok(1)
            } else {
              Err(0)
            };

            if tries.get() < num_retries {
              tries.set(tries.get() + 1);
            }

            result
          })
          .await;

        assert_eq!(Ok(1), result);
        assert_eq!(num_retries, tries.get());
        assert_eq!(num_retries, times_backoff_got_called.get());
      });
    }

    #[test]
    fn can_avoid_retrying_if_error_is_unrecoverable(num_retries in 0..1000_u16) {
      prop_assume!(num_retries > 0);

      Runtime::new().unwrap().block_on(async {
        let cb = CircuitBreaker::new();

        let tries = Rc::new(Cell::new(0_u16));

        let result = cb
          .with_retries(num_retries as u32)
          .exec(|| async {
            if tries.get() < num_retries {
              tries.set(tries.get() + 1);
            }

            RetryResult::<i32, &'static str>::DontRetry("we cant recover from this")
          })
          .await;

        assert_eq!(Err("we cant recover from this"), result);
        assert_eq!(tries.get(), 1);
      });
    }
  }
}
