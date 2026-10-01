//! A Chrome DevTools Protocol client: JSON commands with ids, their replies, and events, over any
//! transport that carries whole messages.

use anyhow::{Result, anyhow};
use collections::HashMap;
use futures::channel::oneshot;
use gpui::{BackgroundExecutor, Task};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// An event Chrome sent, from a tab's session or, when `session_id` is `None`, from the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct CdpEvent {
    pub session_id: Option<String>,
    pub method: String,
    pub params: Value,
}

type PendingReplies = HashMap<u64, oneshot::Sender<Result<Value>>>;

pub struct CdpConnection {
    outgoing: async_channel::Sender<String>,
    next_id: AtomicU64,
    pending: Arc<Mutex<Option<PendingReplies>>>,
    subscribers: Arc<Mutex<Vec<async_channel::Sender<CdpEvent>>>>,
    _reader: Task<()>,
}

impl CdpConnection {
    /// Speaks CDP over `outgoing` and `incoming`, each item one JSON message. The connection
    /// closes when `incoming` ends; every waiting and later command then fails.
    pub fn new(
        outgoing: async_channel::Sender<String>,
        incoming: async_channel::Receiver<String>,
        executor: &BackgroundExecutor,
    ) -> Arc<Self> {
        let pending = Arc::new(Mutex::new(Some(PendingReplies::default())));
        let subscribers = Arc::new(Mutex::new(Vec::<async_channel::Sender<CdpEvent>>::new()));
        let reader = executor.spawn({
            let pending = pending.clone();
            let subscribers = subscribers.clone();
            async move {
                while let Ok(message) = incoming.recv().await {
                    route_message(&message, &pending, &subscribers);
                }
                // Dropping the pending senders fails every waiting command.
                pending.lock().take();
                subscribers.lock().clear();
            }
        });
        Arc::new(Self {
            outgoing,
            next_id: AtomicU64::new(1),
            pending,
            subscribers,
            _reader: reader,
        })
    }

    /// Sends `method` to the browser, or to a tab when `session_id` is set, and waits for the
    /// reply's `result`.
    pub async fn send(
        &self,
        session_id: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply_tx, reply_rx) = oneshot::channel();
        match self.pending.lock().as_mut() {
            Some(pending) => pending.insert(id, reply_tx),
            None => return Err(anyhow!("{method}: the browser has exited")),
        };
        let mut message = serde_json::json!({ "id": id, "method": method, "params": params });
        if let Some(session_id) = session_id {
            message["sessionId"] = session_id.into();
        }
        if self.outgoing.send(message.to_string()).await.is_err() {
            if let Some(pending) = self.pending.lock().as_mut() {
                pending.remove(&id);
            }
            return Err(anyhow!("{method}: the browser has exited"));
        }
        reply_rx
            .await
            .map_err(|_| anyhow!("{method}: the browser has exited"))?
            .map_err(|error| anyhow!("{method}: {error}"))
    }

    /// Every event received from now on.
    pub fn subscribe(&self) -> async_channel::Receiver<CdpEvent> {
        let (events_tx, events_rx) = async_channel::unbounded();
        if self.pending.lock().is_some() {
            self.subscribers.lock().push(events_tx);
        }
        events_rx
    }

    pub fn is_closed(&self) -> bool {
        self.pending.lock().is_none()
    }
}

fn route_message(
    message: &str,
    pending: &Mutex<Option<PendingReplies>>,
    subscribers: &Mutex<Vec<async_channel::Sender<CdpEvent>>>,
) {
    let message: Value = match serde_json::from_str(message) {
        Ok(message) => message,
        Err(error) => {
            log::error!("unreadable message from the browser: {error}");
            return;
        }
    };
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let reply = pending
            .lock()
            .as_mut()
            .and_then(|pending| pending.remove(&id));
        if let Some(reply) = reply {
            let result = match message.get("error") {
                Some(error) => Err(anyhow!(
                    "{}",
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            reply.send(result).ok();
        }
        return;
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    let event = CdpEvent {
        session_id: message
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        method: method.to_string(),
        params: message.get("params").cloned().unwrap_or(Value::Null),
    };
    subscribers
        .lock()
        .retain(|subscriber| subscriber.try_send(event.clone()).is_ok());
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    use gpui::TestAppContext;

    /// The browser's side of a connection: commands zedd sent, and a way to answer them.
    pub struct FakeBrowser {
        pub commands: async_channel::Receiver<String>,
        pub replies: async_channel::Sender<String>,
    }

    impl FakeBrowser {
        pub async fn next_command(&self) -> Value {
            serde_json::from_str(&self.commands.recv().await.unwrap()).unwrap()
        }

        pub async fn reply(&self, command: &Value, result: Value) {
            let reply = serde_json::json!({ "id": command["id"], "result": result });
            self.replies.send(reply.to_string()).await.unwrap();
        }

        pub async fn emit(&self, session_id: Option<&str>, method: &str, params: Value) {
            let mut event = serde_json::json!({ "method": method, "params": params });
            if let Some(session_id) = session_id {
                event["sessionId"] = session_id.into();
            }
            self.replies.send(event.to_string()).await.unwrap();
        }
    }

    pub fn fake_connection(cx: &TestAppContext) -> (Arc<CdpConnection>, FakeBrowser) {
        let (outgoing_tx, outgoing_rx) = async_channel::unbounded();
        let (incoming_tx, incoming_rx) = async_channel::unbounded();
        let connection = CdpConnection::new(outgoing_tx, incoming_rx, &cx.executor());
        (
            connection,
            FakeBrowser {
                commands: outgoing_rx,
                replies: incoming_tx,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fake_connection;
    use gpui::{AppContext as _, TestAppContext};

    #[gpui::test]
    async fn test_replies_reach_their_commands_and_events_reach_subscribers(
        cx: &mut TestAppContext,
    ) {
        let (connection, browser) = fake_connection(cx);
        let events = connection.subscribe();

        let first = cx.background_spawn({
            let connection = connection.clone();
            async move {
                connection
                    .send(
                        Some("tab-1"),
                        "Runtime.evaluate",
                        serde_json::json!({ "expression": "1" }),
                    )
                    .await
            }
        });
        let second = cx.background_spawn({
            let connection = connection.clone();
            async move {
                connection
                    .send(None, "Browser.getVersion", serde_json::json!({}))
                    .await
            }
        });

        let first_command = browser.next_command().await;
        let second_command = browser.next_command().await;
        assert_eq!(first_command["sessionId"], "tab-1");
        assert_eq!(first_command["method"], "Runtime.evaluate");
        assert!(second_command.get("sessionId").is_none());

        // Replies may arrive in any order.
        browser
            .reply(
                &second_command,
                serde_json::json!({ "product": "Chrome/1" }),
            )
            .await;
        browser
            .replies
            .send(
                serde_json::json!({
                    "id": first_command["id"],
                    "error": { "code": -32000, "message": "Cannot find context" },
                })
                .to_string(),
            )
            .await
            .unwrap();
        browser
            .emit(
                Some("tab-1"),
                "Page.loadEventFired",
                serde_json::json!({ "timestamp": 1 }),
            )
            .await;

        assert_eq!(
            second.await.unwrap(),
            serde_json::json!({ "product": "Chrome/1" })
        );
        assert_eq!(
            first.await.unwrap_err().to_string(),
            "Runtime.evaluate: Cannot find context"
        );
        assert_eq!(
            events.recv().await.unwrap(),
            CdpEvent {
                session_id: Some("tab-1".to_string()),
                method: "Page.loadEventFired".to_string(),
                params: serde_json::json!({ "timestamp": 1 }),
            }
        );
    }

    #[gpui::test]
    async fn test_commands_fail_once_the_browser_exits(cx: &mut TestAppContext) {
        let (connection, browser) = fake_connection(cx);
        let waiting = cx.background_spawn({
            let connection = connection.clone();
            async move {
                connection
                    .send(None, "Page.navigate", serde_json::json!({}))
                    .await
            }
        });
        browser.next_command().await;
        drop(browser);
        cx.run_until_parked();

        assert_eq!(
            waiting.await.unwrap_err().to_string(),
            "Page.navigate: the browser has exited"
        );
        assert!(connection.is_closed());
        assert_eq!(
            connection
                .send(None, "Page.reload", serde_json::json!({}))
                .await
                .unwrap_err()
                .to_string(),
            "Page.reload: the browser has exited"
        );
    }
}
