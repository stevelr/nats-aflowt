// Copyright 2020-2022 The NATS Authors
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{client::Client, message::Message, Stream};
use std::{io, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Debug)]
struct Inner {
    /// Subscription ID.
    pub(crate) sid: u64,

    /// Subject.
    #[allow(dead_code)]
    pub(crate) subject: String,

    /// MSG operations received from the server.
    pub(crate) messages: SubscriptionReceiver<Message>,

    /// Client associated with subscription.
    pub(crate) client: Client,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let client = self.client.clone();
        let sid = self.sid;
        tokio::spawn(async move {
            client.unsubscribe(sid).await.ok();
        });
    }
}

/// Wrapper around `tokio::sync::mpsc::Receiver` that provides interior mutability
#[derive(Debug)]
pub struct SubscriptionReceiver<T> {
    inner: Mutex<tokio::sync::mpsc::Receiver<T>>,
}

impl<T> SubscriptionReceiver<T> {
    /// Receives the next value. Returns None if the channel has been closed
    /// and there are no more values.
    pub async fn recv(&self) -> Option<T> {
        let mut receiver = self.inner.lock().await;
        let x = receiver.recv().await;
        x
    }

    /// Return Some(message) if a message is available,
    /// or None if there are no messages available,
    /// or the subscription has been closed or client disconnected.
    pub async fn try_recv(&self) -> Option<T> {
        let mut receiver =
            match tokio::time::timeout(Duration::from_secs(10), self.inner.lock()).await {
                Err(_) => {
                    panic!("try_recv in subscription failed to get inner lock in 10 secs");
                }
                Ok(g) => g,
            };
        //let mut receiver = self.inner.lock().await;
        match receiver.try_recv() {
            Ok(m) => Some(m),
            Err(_) => None,
        }
    }
}

impl<T> From<tokio::sync::mpsc::Receiver<T>> for SubscriptionReceiver<T> {
    fn from(r: tokio::sync::mpsc::Receiver<T>) -> Self {
        Self {
            inner: Mutex::new(r),
        }
    }
}

/// A `Subscription` receives `Message`s published
/// to specific NATS `Subject`s.
#[derive(Clone, Debug)]
pub struct Subscription(Arc<Inner>);

impl Subscription {
    /// Creates a subscription.
    pub(crate) fn new(
        sid: u64,
        subject: String,
        messages: SubscriptionReceiver<Message>,
        client: Client,
    ) -> Subscription {
        Subscription(Arc::new(Inner {
            sid,
            subject,
            messages,
            client,
        }))
    }

    /// Get a Receiver for subscription messages.
    /// Useful for `tokio::select` macro
    ///
    /// # Example
    /// ```
    /// # use std::io;
    /// # use nats_aflowt::Subscription;
    /// # // test helper function to create a subscription and publish one item to it
    /// # async fn sub_with_item(nc: &nats_aflowt::Connection,msg: &str) -> std::io::Result<nats_aflowt::Subscription> {
    /// #     let name = format!("sub_{}", rand::random::<u64>());
    /// #     let sub = nc.subscribe(name.as_str()).await?;
    /// #     nc.publish(&name, msg).await?;
    /// #     Ok(sub)
    /// # }
    /// # #[tokio::main]
    /// # async fn main() -> io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// # let sub1 = sub_with_item(&nc, "hello").await?;
    /// # let sub2 = sub_with_item(&nc, "howdy").await?;
    /// let sub1_ch = sub1.receiver();
    /// let sub2_ch = sub2.receiver();
    /// for _ in 0..=1 {
    ///     tokio::select! {
    ///         msg = sub1_ch.recv() => {
    ///             println!("Got message from sub1: {:?}", msg);
    ///         }
    ///         msg = sub2_ch.recv() => {
    ///             println!("Got message from sub2: {:?}", msg);
    ///         }
    ///     }
    /// }
    /// # assert!(sub1_ch.try_recv().await.is_none());
    /// # assert!(sub2_ch.try_recv().await.is_none());
    /// # Ok(())
    /// # }
    /// ```
    pub fn receiver(&self) -> &SubscriptionReceiver<Message> {
        &self.0.messages
    }

    /// Get (wait for) the next message, or None if the subscription
    /// has been unsubscribed or the connection closed.
    ///
    /// # Example
    /// ```
    /// # // test helper function to create a subscription and publish one item to it
    /// # async fn sub_with_item(nc: &nats_aflowt::Connection,msg: &str) -> std::io::Result<nats_aflowt::Subscription> {
    /// #     let name = format!("sub_{}", rand::random::<u64>());
    /// #     let sub = nc.subscribe(name.as_str()).await?;
    /// #     nc.publish(&name, msg).await?;
    /// #     Ok(sub)
    /// # }
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// # let sub = sub_with_item(&nc, "hello").await?;
    /// if let Some(msg) = sub.next().await {
    ///     println!("Received: {}", msg);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn next(&self) -> Option<Message> {
        self.0.messages.recv().await
    }

    /// Try to get the next message, or None if no messages
    /// are present or if the subscription has been unsubscribed
    /// or the connection closed.
    ///
    /// # Example
    /// ```
    /// # // test helper function to create a subscription and publish one item to it
    /// # async fn sub_with_item(nc: &nats_aflowt::Connection,msg: &str) -> std::io::Result<nats_aflowt::Subscription> {
    /// #     let name = format!("sub_{}", rand::random::<u64>());
    /// #     let sub = nc.subscribe(name.as_str()).await?;
    /// #     nc.publish(&name, msg).await?;
    /// #     Ok(sub)
    /// # }
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// # let sub = sub_with_item(&nc, "hello").await?;
    /// if let Some(msg) = sub.try_next().await {
    ///   println!("Received {}", msg);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn try_next(&self) -> Option<Message> {
        self.0.messages.try_recv().await
    }

    /// Get the next message, or a timeout error
    /// if no messages are available for timout.
    ///
    /// # Example
    /// ```
    /// # // test helper function to create a subscription and publish one item to it
    /// # async fn sub_with_item(nc: &nats_aflowt::Connection,msg: &str) -> std::io::Result<nats_aflowt::Subscription> {
    /// #     let name = format!("sub_{}", rand::random::<u64>());
    /// #     let sub = nc.subscribe(name.as_str()).await?;
    /// #     nc.publish(&name, msg).await?;
    /// #     Ok(sub)
    /// # }
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// # let sub = sub_with_item(&nc, "hello").await?;
    /// if let Ok(message) = sub.next_timeout(std::time::Duration::from_secs(1)).await {
    ///     println!("Received {}", message);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn next_timeout(&self, timeout: Duration) -> io::Result<Message> {
        match tokio::time::timeout(timeout, self.0.messages.recv()).await {
            Ok(Some(msg)) => Ok(msg),
            Ok(None) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "next_timeout: timed out",
            )),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "next_timeout: unsubscribed",
            )),
        }
    }

    /// Returns a pinned message stream.
    /// same as `stream()`
    ///
    /// # Example
    /// ```no_run
    /// use futures::stream::StreamExt;
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let mut sub = nc.subscribe("foo").await?.messages();
    /// while let Some(msg) = sub.next().await {
    ///    // ...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn messages(self) -> Pin<Box<dyn Stream<Item = Message>>> {
        Box::pin(self.into_stream())
    }

    /// Returns a stream (unpinned)
    #[doc(hidden)]
    fn into_stream(self) -> impl Stream<Item = Message> {
        async_stream::stream! {
            while let Some(message) = self.next().await {
                yield message;
            }
        }
    }

    /// Returns a pinned message stream.
    ///
    /// # Example
    /// ```no_run
    /// use futures::stream::StreamExt;
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let mut sub = nc.subscribe("foo").await?.stream();
    /// while let Some(msg) = sub.next().await {
    ///    // ...
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn stream(self) -> Pin<Box<dyn Stream<Item = Message>>> {
        Box::pin(self.into_stream())
    }

    /// Attach a closure to handle messages. This closure will execute in a
    /// separate thread. The result of this call is a `Handler` which can
    /// not be iterated and must be unsubscribed or closed directly to
    /// unregister interest. A `Handler` will not unregister interest with
    /// the server when `drop(&mut self)` is called.
    ///
    /// # Example
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// nc.subscribe("bar").await?.with_handler(move |msg| {
    ///     println!("Received {}", &msg);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_handler<F>(self, handler: F) -> Handler
    where
        F: Fn(Message) -> io::Result<()> + Send + Sync + 'static,
    {
        let sub = self.clone();
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            while let Some(m) = sub.next().await {
                let handler = handler.clone();
                // just in case the handler blocks, we need to use blocking thread pool
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(e) = handler(m) {
                        // TODO(dlc) - Capture for last error?
                        log::error!("Error in callback! {:?}", e);
                    }
                });
            }
        });
        // This will allow us to not have to capture the return. When it is
        // dropped it will not unsubscribe from the server.
        Handler { sub: self }
    }

    /// Attach an async closure to handle messages. The closure will run as a task
    /// within the current thread and must not be blocking.
    /// Any errors returned by the closure will be logged.
    /// A `Handler` will not unregister interest with
    /// the server when `drop(&mut self)` is called.
    ///
    /// # Example
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// # let name = format!("sub_{}", rand::random::<u64>());
    /// let sub = nc.subscribe(name.as_str()).await?
    ///      .with_async_handler( move |m| async move { m.respond(b"ans=42").await?; Ok(()) });
    /// #
    /// # let resp = nc.request(&name, "send answer").await?;
    /// # assert_eq!(resp.data, b"ans=42");
    /// # Ok(())
    /// # }
    /// ```
    #[allow(unknown_lints, clippy::return_self_not_must_use)]
    pub fn with_async_handler<F, T>(self, handler: F) -> Self
    where
        F: Fn(Message) -> T + 'static + Send + Sync,
        T: futures::Future<Output = io::Result<()>> + Send,
    {
        let sub = self.clone();
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            while let Some(m) = sub.next().await {
                let handler = handler.clone();
                let _ = tokio::spawn(async move {
                    if let Err(e) = handler(m).await {
                        // TODO(dlc) - Capture for last error?
                        log::error!("Error in callback! {:?}", e);
                    }
                });
            }
        });
        self
    }

    /// Unsubscribe a subscription immediately without draining.
    /// Use `drain` instead if you want any pending messages
    /// to be processed by a handler, if one is configured.
    ///
    /// # Example
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let sub = nc.subscribe("foo").await?;
    /// sub.unsubscribe().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unsubscribe(self) -> io::Result<()> {
        self.drain().await?;
        // Discard all queued messages.
        while self.0.messages.try_recv().await.is_some() {}
        Ok(())
    }

    /// Close a subscription. Same as `unsubscribe`
    ///
    /// Use `drain` instead if you want any pending messages
    /// to be processed by a handler, if one is configured.
    ///
    /// # Example
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let sub = nc.subscribe("foo").await?;
    /// sub.close().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn close(self) -> io::Result<()> {
        self.unsubscribe().await
    }

    /// Send an unsubscription then flush the connection,
    /// allowing any unprocessed messages to be handled
    /// by a handler function if one is configured.
    ///
    /// After the flush returns, we know that a round-trip
    /// to the server has happened after it received our
    /// unsubscription, so we shut down the subscriber
    /// afterwards.
    ///
    /// A similar method exists on the `Connection` struct
    /// which will drain all subscriptions for the NATS
    /// client, and transition the entire system into
    /// the closed state afterward.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::sync::{Arc, atomic::{AtomicBool, Ordering::SeqCst}};
    /// # use std::thread;
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let mut sub = nc.subscribe("test.drain").await?;
    ///
    /// nc.publish("test.drain", "message").await?;
    /// sub.drain().await?;
    ///
    /// let has_item = sub.next().await.is_some();
    /// assert!(has_item);
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub async fn drain(&self) -> io::Result<()> {
        self.0.client.flush(crate::DEFAULT_FLUSH_TIMEOUT).await?;
        self.0.client.unsubscribe(self.0.sid).await?;
        Ok(())
    }
}

/// A `Handler` may be used to unsubscribe a handler thread.
pub struct Handler {
    sub: Subscription,
}

impl Handler {
    /// Unsubscribe a subscription.
    ///
    /// # Example
    /// ```
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// # let nc = nats_aflowt::connect("127.0.0.1:14222").await?;
    /// let sub = nc.subscribe("foo").await?.with_handler(move |msg| {
    ///     println!("Received {}", &msg);
    ///     Ok(())
    /// });
    /// sub.unsubscribe().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unsubscribe(self) -> io::Result<()> {
        self.sub.drain().await
    }
}
