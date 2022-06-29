use std::{
  future::Future,
  sync::atomic::{AtomicI8, Ordering},
};

const CLOSED: i8 = 0;
const OPEN: i8 = 1;
const HALF_OPEN: i8 = 2;

pub struct CircuitBreaker {
  /// Is the circuit breaker closed, open or half open?
  state: AtomicI8,
  /// The number of retries before considering a call as failed.
  retries: u32,
}

impl CircuitBreaker {
  pub fn new() -> Self {
    Self {
      state: AtomicI8::new(CLOSED),
      retries: 1,
    }
  }

  pub fn retries(&mut self, n: u32) -> &mut Self {
    assert!(n > 0, "retries must be greater than 0");
    todo!()
  }

  pub fn is_open(&self) -> bool {
    self.state.load(Ordering::Relaxed) == OPEN
  }

  pub fn is_half_open(&self) -> bool {
    self.state.load(Ordering::Relaxed) == HALF_OPEN
  }

  pub fn is_closed(&self) -> bool {
    self.state.load(Ordering::Relaxed) == CLOSED
  }

  pub fn open(&self) {
    self.state.store(OPEN, Ordering::Release);
  }

  pub fn half_open(&self) {
    self.state.store(HALF_OPEN, Ordering::Release);
  }

  pub fn close(&self) {
    self.state.store(CLOSED, Ordering::Release);
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
  pub async fn exec<F, T, E, Fut, Out>(&self, f: F) -> Out
  where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Out>,
    Out: Into<ExecResult<T, E>>,
  {
    f().await
  }
}

#[cfg(test)]
mod tests {
  use retry::FullJitterExponentialBackoff;

  use super::*;

  #[test]
  #[should_panic(expected = "retries must be greater than 0")]
  fn retries_must_be_greater_than_0() {
    CircuitBreaker::new().retries(0);
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

  #[tokio::test]
  async fn can_retry_before_considering_a_call_as_failed() {
    let cb = CircuitBreaker::new();

    let result = cb
      .retries(3, FullJitterExponentialBackoff::recommended())
      .exec_with(|| async {
        // ...
        Result::<i32, i32>::Ok(1)
      })
      .await;

    dbg!(&result);
  }
}
