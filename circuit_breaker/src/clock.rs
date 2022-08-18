use std::time::{Duration, Instant};

pub trait Clock {
  fn time_elapsed_since_ms(since: Instant) -> Duration;
}
