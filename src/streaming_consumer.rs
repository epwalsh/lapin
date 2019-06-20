use crate::{
  confirmation::NotifyReady,
  consumer::ConsumerSubscriber,
  message::Delivery,
  types::ShortString,
};

use log::trace;
use parking_lot::{Mutex, MutexGuard};

use std::{
  collections::VecDeque,
  fmt,
  sync::Arc,
};

#[derive(Clone, Default)]
pub struct StreamingConsumer {
  inner: Arc<Mutex<ConsumerInner>>,
}

impl StreamingConsumer {
  pub fn inner(&self) -> MutexGuard<'_, ConsumerInner> {
    self.inner.lock()
  }
}

pub struct ConsumerInner {
  deliveries: VecDeque<Delivery>,
  task:       Option<Box<dyn NotifyReady + Send>>,
  canceled:   bool,
  tag:        ShortString,
}

impl ConsumerInner {
  pub fn next_delivery(&mut self) -> Option<Delivery> {
    self.deliveries.pop_front()
  }

  pub fn set_task(&mut self, task: Box<dyn NotifyReady + Send>) {
    self.task = Some(task);
  }

  pub fn has_task(&self) -> bool {
    self.task.is_some()
  }

  pub fn canceled(&self) -> bool {
    self.canceled
  }

  pub fn set_tag(&mut self, consumer_tag: ShortString) {
    self.tag = consumer_tag;
  }

  pub fn tag(&self) -> &ShortString {
    &self.tag
  }
}

impl Default for ConsumerInner {
  fn default() -> Self {
    Self {
      deliveries: VecDeque::new(),
      task:       None,
      canceled:   false,
      tag:        ShortString::default(),
    }
  }
}

impl ConsumerSubscriber for StreamingConsumer {
  fn new_delivery(&self, delivery: Delivery) {
    trace!("new_delivery;");
    let mut inner = self.inner.lock();
    inner.deliveries.push_back(delivery);
    if let Some(task) = inner.task.as_ref() {
      task.notify();
    }
  }
  fn drop_prefetched_messages(&self) {
    trace!("drop_prefetched_messages;");
    self.inner.lock().deliveries.clear();
  }
  fn cancel(&self) {
    trace!("cancel;");
    let mut inner = self.inner.lock();
    inner.deliveries.clear();
    inner.canceled = true;
    inner.task.take();
  }
}

impl fmt::Debug for StreamingConsumer {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "StreamingConsumer({})", self.inner().tag())
  }
}

#[cfg(feature = "futures")]
mod futures {
  use super::*;

  use ::futures::{
    stream::Stream,
    task::{Context, Poll},
  };

  use std::pin::Pin;

  use crate::confirmation::futures::Watcher;

  impl Stream for StreamingConsumer {
    type Item = Delivery;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
      trace!("consumer poll; polling transport");
      let mut inner = self.inner();
      trace!("consumer poll; acquired inner lock, consumer_tag={}", inner.tag());
      if !inner.has_task() {
        inner.set_task(Box::new(Watcher(cx.waker().clone())));
      }
      if let Some(delivery) = inner.next_delivery() {
        trace!("delivery; consumer_tag={}, delivery_tag={:?}", inner.tag(), delivery.delivery_tag);
        Poll::Ready(Some(delivery))
      } else if inner.canceled() {
        trace!("consumer canceled; consumer_tag={}", inner.tag());
        Poll::Ready(None)
      } else {
        trace!("delivery; status=NotReady, consumer_tag={}", inner.tag());
        Poll::Pending
      }
    }
  }
}
