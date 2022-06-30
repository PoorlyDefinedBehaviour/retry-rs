use std::{
  future::Future,
  sync::{
    atomic::{AtomicI8, Ordering},
    Arc,
  },
};

use retry::Backoff;

const CLOSED: i8 = 0;
const OPEN: i8 = 1;
const HALF_OPEN: i8 = 2;

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
}

impl CircuitBreaker {
  pub fn new() -> Self {
    Self {
      inner: Arc::new(CircuitBreakerInner {
        // Circuit starts closed.
        state: AtomicI8::new(CLOSED),
      }),
      /// Will try to execute the closure passed to exec only once by default.
      retries: 1,
      backoff: None,
    }
  }

  /// Sets the number of retries for the next call.
  pub fn with_retries(&self, n: u32) -> Self {
    assert!(n > 0, "retries must be greater than 0");
    Self {
      inner: Arc::clone(&self.inner),
      retries: n,
      backoff: self.backoff.clone(),
    }
  }

  /// Sets backoff strategy for the next call.
  pub fn with_backoff(&self, backoff: impl Backoff + Send + Sync + 'static) -> Self {
    Self {
      inner: Arc::clone(&self.inner),
      backoff: Some(Arc::new(backoff)),
      ..*self
    }
  }

  /// Returns true when the circuit is open.
  pub fn is_open(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == OPEN
  }

  // Returns true when the circuit is half open.
  pub fn is_half_open(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == HALF_OPEN
  }

  /// Returns true when the circuit is closed.
  pub fn is_closed(&self) -> bool {
    self.inner.state.load(Ordering::Relaxed) == CLOSED
  }

  /// Opens the circuit.
  pub fn open(&self) {
    self.inner.state.store(OPEN, Ordering::Release);
  }

  /// Transitions the circuit to half open.
  pub fn half_open(&self) {
    self.inner.state.store(HALF_OPEN, Ordering::Release);
  }

  /// Closes the circuit.
  pub fn close(&self) {
    self.inner.state.store(CLOSED, Ordering::Release);
  }
}

pub enum ExecResult<T, E> {
  Err(E),
  Ok(T),
}

impl<T, E> From<Result<T, E>> for ExecResult<T, E> {
  fn from(input: Result<T, E>) -> ExecResult<T, E> {
    match input {
      Err(err) => ExecResult::Err(err),
      Ok(err) => ExecResult::Ok(err),
    }
  }
}

impl CircuitBreaker {
  pub async fn exec<F, T, E, Fut, Out>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> Fut,
    Fut: Future<Output = Out>,
    Out: Into<ExecResult<T, E>>,
  {
    let mut retry = 0;
    loop {
      match f().await.into() {
        ExecResult::Err(err) => {
          if retry == self.retries {
            return Err(err);
          }

          if let Some(ref backoff) = self.backoff {
            backoff.wait(retry).await;
          }
        }
        ExecResult::Ok(value) => return Ok(value),
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
  }
}
