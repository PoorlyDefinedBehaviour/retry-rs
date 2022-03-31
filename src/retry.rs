use async_trait::async_trait;
use std::{cell::RefCell, fmt::Debug, future::Future};

use tracing::error;

struct NoOpBackoff;

#[async_trait]
impl Backoff for NoOpBackoff {
  async fn wait(&mut self, _retry: usize) {}
}

pub struct Retry {
  retries: usize,
  backoff: RefCell<Box<dyn Backoff>>,
}

#[async_trait]
pub trait Backoff {
  async fn wait(&mut self, retry: usize);
}

impl Default for Retry {
  fn default() -> Self {
    Self {
      retries: 1,
      backoff: RefCell::new(Box::new(NoOpBackoff)),
    }
  }
}

impl Retry {
  pub fn retries(&mut self, n: usize) -> &mut Self {
    assert!(n > 0, "retries must be greater than 0");
    self.retries = n;
    self
  }

  pub fn backoff<B>(&mut self, b: B) -> &mut Self
  where
    B: 'static + Backoff,
  {
    self.backoff = RefCell::new(Box::new(b));
    self
  }

  pub async fn exec<T, E, F, Out>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> Out,
    Out: Future<Output = Result<T, E>>,
    E: Debug,
  {
    let mut tries = 0;

    loop {
      match f().await {
        Err(err) => {
          tries += 1;

          error!(retries = self.retries, ?tries, ?err);

          if tries >= self.retries {
            return Err(err);
          }

          self.backoff.borrow_mut().wait(tries).await;
        }
        Ok(value) => return Ok(value),
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
    Retry::default().retries(0);
  }

  #[tokio::test]
  async fn retries_defaults_to_1() {
    let tries = Rc::new(Cell::new(0));
    let result: Result<i32, &str> = Retry::default()
      .exec(|| async {
        tries.set(tries.get() + 1);
        Err("oops")
      })
      .await;
    assert_eq!(Err("oops"), result);
    assert_eq!(1, tries.get());
  }

  proptest! {
    #[test]
    fn exec_succeeds_on_first_try(num_retries in 1..=1000_usize) {
      Runtime::new().unwrap().block_on(async {
        let result: Result<i32, String> = Retry::default().retries(num_retries).exec(||async{ Ok(1)}).await;
        assert_eq!(Ok(1), result);
      });
    }

    #[test]
    fn error_is_returned_if_task_never_succeeds(num_retries in 1..=1000_usize) {
      Runtime::new().unwrap().block_on(async {
        let result: Result<i32, &str> = Retry::default().retries(num_retries).exec(||async { Err("nope")}).await;
        assert_eq!(Err("nope"), result);
      });
    }

    #[test]
    fn exec_succeeds_on_nth_retry(num_retries in 1..=1000_usize) {
      Runtime::new().unwrap().block_on(async {
        let tries = Rc::new(Cell::new(0));


        let result = Retry::default().retries(num_retries).exec(|| async {
          tries.set(tries.get()+1);

          if tries.get() == num_retries {
            Ok(1)
          } else {
            Err("oops")
          }
        }).await;

        assert_eq!(Ok(1), result);
        assert_eq!(num_retries, tries.get());
      });
    }
  }
}
