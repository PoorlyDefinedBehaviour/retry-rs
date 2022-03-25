use std::fmt::Debug;

use tracing::error;

struct Retry {
  retries: usize,
}

enum RetryResult<T, E> {
  Done(T),
  Error(E),
}

impl Retry {
  pub fn new() -> Self {
    Retry { retries: 1 }
  }

  pub fn retries(&self, n: usize) -> Self {
    assert!(n > 0, "retries must be greater than 0");
    Self {
      retries: n,
      ..*self
    }
  }

  pub fn exec<T, E, F>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> Result<T, E>,
    E: Debug,
  {
    self.exec_with(|| match f() {
      Err(err) => RetryResult::Error(err),
      Ok(value) => RetryResult::Done(value),
    })
  }

  pub fn exec_with<T, E, F>(&self, mut f: F) -> Result<T, E>
  where
    F: FnMut() -> RetryResult<T, E>,
    E: Debug,
  {
    let mut tries = 0;

    loop {
      match f() {
        RetryResult::Error(err) => {
          tries += 1;

          error!(retries = self.retries, ?tries, ?err);

          if tries >= self.retries {
            return Err(err);
          }
        }
        RetryResult::Done(value) => return Ok(value),
      }
    }
  }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  // let x = Retry::new().retries(3).exec(|| 1)?;

  // dbg!(x);

  Ok(())
}

#[cfg(test)]
mod tests {

  use std::sync::Mutex;

  use super::*;

  #[test]
  #[should_panic(expected = "retries must be greater than 0")]
  fn retries_must_be_greater_than_0() {
    Retry::new().retries(0);
  }

  #[test]
  fn retries_defaults_to_1() {
    let mut tries = 0;
    let result: Result<i32, &str> = Retry::new().exec(|| {
      tries += 1;
      Err("oops")
    });
    assert_eq!(Err("oops"), result);
    assert_eq!(1, tries);
  }

  #[test]
  fn exec_succeeds_on_first_try() {
    let result: Result<i32, String> = Retry::new().retries(3).exec(|| Ok(1));
    assert_eq!(Ok(1), result);
  }

  #[test]
  fn exec_succeeds_on_second_try() {
    let mut tries = 0;

    let result = Retry::new().retries(3).exec(|| {
      tries += 1;

      if tries > 1 {
        Ok(1)
      } else {
        Err("oops")
      }
    });

    assert_eq!(Ok(1), result);
    assert_eq!(2, tries);
  }
}
