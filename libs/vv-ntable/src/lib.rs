use futures_util::{SinkExt, StreamExt};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tracing::Instrument;
use triomphe::Arc;

mod protocol;
pub mod team;
pub mod value;

#[derive(Debug)]
enum Topic {
    String(SmolStr),
    Uid(u32),
}

#[derive(Debug)]
struct PublishMessage {
    topic: Topic,
    timestamp: u64,
    datatype: value::DataType,
    type_str: &'static str,
    data: Vec<u8>,
    cache: Option<Arc<AtomicU32>>,
}

#[derive(Debug, Clone)]
pub struct NtHandle {
    pub_tx: mpsc::UnboundedSender<PublishMessage>,
}
impl NtHandle {
    pub fn set_erased(
        &self,
        topic: impl Into<SmolStr>,
        datatype: value::DataType,
        type_str: &'static str,
        data: Vec<u8>,
    ) {
        let now = SystemTime::now();
        let offset = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .as_ref()
            .map_or(0, Duration::as_micros);
        let _ = self.pub_tx.send(PublishMessage {
            topic: Topic::String(topic.into()),
            timestamp: offset as _,
            datatype,
            type_str,
            data,
            cache: None,
        });
    }
    pub fn set<T: value::GenericType>(&self, topic: impl Into<SmolStr>, val: T) {
        let mut buf = Vec::new();
        val.serialize_rmp(&mut buf);
        self.set_erased(topic.into(), val.data_type(), val.type_string(), buf);
    }
    pub fn generic_publish(&self, topic: impl Into<SmolStr>) -> GenericPublisher {
        GenericPublisher {
            pub_tx: self.pub_tx.clone(),
            topic: topic.into(),
            cache: Arc::new(AtomicU32::new(0)),
        }
    }
    pub fn publish<T: value::ConcreteType>(&self, topic: impl Into<SmolStr>) -> Publisher<T> {
        Publisher {
            inner: self.generic_publish(topic.into()),
            _marker: PhantomData,
        }
    }
}

/// A typeless publisher.
#[derive(Debug, Clone)]
pub struct GenericPublisher {
    pub_tx: mpsc::UnboundedSender<PublishMessage>,
    topic: SmolStr,
    cache: Arc<AtomicU32>,
}
impl GenericPublisher {
    pub fn set_erased(&self, datatype: value::DataType, type_str: &'static str, data: Vec<u8>) {
        let now = SystemTime::now();
        let offset = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .as_ref()
            .map_or(0, Duration::as_micros);
        let cached = self.cache.load(Ordering::Relaxed);
        let (topic, cache) = if cached == 0 {
            (Topic::String(self.topic.clone()), Some(self.cache.clone()))
        } else {
            (Topic::Uid(cached), None)
        };
        let _ = self.pub_tx.send(PublishMessage {
            topic,
            timestamp: offset as _,
            datatype,
            type_str,
            data,
            cache,
        });
    }
    pub fn set<T: value::GenericType>(&self, val: T) {
        let mut buf = Vec::new();
        val.serialize_rmp(&mut buf);
        self.set_erased(val.data_type(), val.type_string(), buf);
    }
}

pub struct Publisher<T> {
    inner: GenericPublisher,
    _marker: PhantomData<fn(T)>,
}
impl<T: value::ConcreteType> Publisher<T> {
    pub fn set(&self, val: &T) {
        let mut buf = Vec::new();
        val.serialize_rmp(&mut buf);
        self.inner.set_erased(
            <T as value::ConcreteType>::data_type(),
            <T as value::ConcreteType>::type_string(),
            buf,
        );
    }
}

#[derive(Debug)]
pub struct NtClient {
    pub identity: String,
    pub port: u16,
    /// How often to send a keepalive ping.
    ///
    /// The default is 500ms
    pub keepalive: Duration,
    handle: NtHandle,
    pub_rx: mpsc::UnboundedReceiver<PublishMessage>,
    pubuids: HashMap<SmolStr, u32>,
    types: Vec<&'static str>,
    time_offset: i64,
}
impl NtClient {
    pub fn new(identity: String) -> Self {
        let (pub_tx, pub_rx) = mpsc::unbounded_channel();
        Self {
            identity,
            port: 5810,
            keepalive: Duration::from_millis(500),
            pub_rx,
            handle: NtHandle { pub_tx },
            pubuids: HashMap::new(),
            types: Vec::new(),
            time_offset: 0,
        }
    }
    pub fn handle(&self) -> &NtHandle {
        &self.handle
    }
    /// Connect to the given host.
    ///
    /// If the connection is dropped and `reconnect` is set to true, it reconnects on failure.
    pub async fn connect<H: Display>(
        &mut self,
        host: H,
        reconnect: bool,
    ) -> tungstenite::Result<()> {
        use std::fmt::Write;
        let mut msg = "ws://".to_string();
        let _ = write!(msg, "{host}");
        let end = msg.len();
        let _ = write!(msg, ":{}/nt/{}", self.port, self.identity);
        let span = tracing::error_span!(
            "connect",
            host = &msg[5..end],
            identity = self.identity,
            port = self.port
        );
        let uri: tungstenite::http::Uri = msg.try_into().expect("Invalid URI");
        tracing::info!(parent: &span, %uri, "resolved URI");
        let req = tungstenite::ClientRequestBuilder::new(uri)
            .with_sub_protocol("v4.1.networktables.first.wpi.edu");
        let mut lce = true;
        loop {
            let res = self
                .connect_req(req.clone(), &mut lce)
                .instrument(span.clone())
                .await;
            if !reconnect {
                return res;
            }
            tokio::time::sleep(Duration::from_millis(100)).await; // hopefully long enough for whatever issue there was to disappear
        }
    }
    async fn connect_req(
        &mut self,
        req: tungstenite::ClientRequestBuilder,
        log_connect_error: &mut bool,
    ) -> tungstenite::Result<()> {
        let (ws, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .inspect_err(|err| {
                if std::mem::take(log_connect_error) {
                    tracing::error!(%err, "failed to connect")
                }
            })?;
        *log_connect_error = true;
        tracing::info!("successfully connected to server");
        let (mut ws_tx, _ws_rx) = ws.split();
        if !self.types.is_empty() {
            let mut msg = Vec::with_capacity(1 + 42 * self.types.len()); // 24 bytes for field names, 8 for quotes, 4 for colons, 4 for commas (including trailing), 2 for braces = a minimum of 42 bytes/entry, +2 because of the surrounding brackets, -1 because no trailing comma
            msg.push(b'[');
            for (name, &pubuid) in &self.pubuids {
                let r#type = self.types[pubuid as usize - 1];
                let _ = serde_json::to_writer(
                    &mut msg,
                    &protocol::ClientToServerMessage::Publish {
                        name,
                        pubuid,
                        r#type,
                        properties: protocol::EmptyMap,
                    },
                );
                msg.push(b',');
            }
            *msg.last_mut().unwrap() = b']';
            ws_tx
                .send(tungstenite::Message::Text(unsafe {
                    tungstenite::Utf8Bytes::from_bytes_unchecked(
                        tungstenite::Bytes::copy_from_slice(&msg),
                    )
                }))
                .await
                .inspect_err(|err| tracing::error!(%err, "failed to re-announce topics"))?;
        }
        // let read_loop = async move {
        //     while let Some(msg) = ws_rx.next().await {
        //         let msg = msg?;
        //         if matches!(msg, tungstenite::Message::Pong(_)) {
        //             continue;
        //         }
        //         tracing::debug!(?msg, "got server message");
        //     }
        //     Ok::<_, tungstenite::Error>(())
        // };
        let write_loop = async move {
            let mut string = Vec::new();
            let mut buf = Vec::with_capacity(8);
            buf.push(0x94);
            let mut messages = Vec::new();
            let mut outgoing = Vec::new();
            loop {
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    self.pub_rx.recv_many(&mut messages, 128),
                )
                .await
                {
                    Ok(0) => break,
                    Ok(_) => {
                        for msg in messages.drain(..) {
                            buf.truncate(1);
                            let pubuid = match msg.topic {
                                Topic::Uid(id) => id,
                                Topic::String(topic) => {
                                    let default = |key: &SmolStr| {
                                        self.types.push(msg.type_str);
                                        let pubuid = self.types.len() as u32;
                                        string.clear();
                                        let _ = serde_json::to_writer(
                                            &mut string,
                                            &[protocol::ClientToServerMessage::Publish {
                                                name: key,
                                                pubuid,
                                                r#type: msg.type_str,
                                                properties: protocol::EmptyMap,
                                            }],
                                        );
                                        tracing::debug!(name = &**key, pubuid, type = msg.type_str, "publishing new topic");
                                        outgoing.push(tungstenite::Message::Text(unsafe {
                                            tungstenite::Utf8Bytes::from_bytes_unchecked(
                                                tungstenite::Bytes::copy_from_slice(&string),
                                            )
                                        }));
                                        pubuid
                                    };
                                    let id = *self.pubuids.entry(topic).or_insert_with_key(default);
                                    if let Some(cache) = msg.cache {
                                        cache.store(id, Ordering::Relaxed);
                                    }
                                    id
                                }
                            };
                            let exp_type = self.types[pubuid as usize - 1];
                            if msg.type_str != exp_type {
                                tracing::warn!(
                                    pubuid,
                                    old = exp_type,
                                    new = msg.type_str,
                                    "message type changed"
                                );
                                continue;
                            }
                            let _ = rmp::encode::write_uint(&mut buf, pubuid as _);
                            let _ = rmp::encode::write_uint(
                                &mut buf,
                                msg.timestamp.strict_add_signed(self.time_offset),
                            );
                            buf.push(msg.datatype as _);
                            buf.extend_from_slice(&msg.data);
                            outgoing.push(tungstenite::Message::Binary(
                                tungstenite::Bytes::copy_from_slice(&buf),
                            ));
                        }
                        for msg in outgoing.drain(..) {
                            ws_tx.feed(msg).await.inspect_err(
                                |err| tracing::error!(%err, "failed to feed message"),
                            )?;
                        }
                        ws_tx
                            .flush()
                            .await
                            .inspect_err(|err| tracing::error!(%err, "failed to flush messages"))?;
                    }
                    Err(_) => {
                        ws_tx
                            .send(tungstenite::Message::Ping((&[] as &[u8]).into()))
                            .await
                            .inspect_err(|err| tracing::error!(%err, "failed to send heartbeat"))?;
                    }
                }
            }
            Ok::<_, tungstenite::Error>(())
        };
        // tokio::select! {
        //     res = read_loop => res,
        //     res = write_loop => res,
        // }
        write_loop.await
    }
    /// Make this client the global client, if none is already set.
    pub fn and_make_global(self) -> Self {
        let _ = GLOBAL_HANDLE.get_or_init(|| self.handle.clone());
        self
    }
    /// Connect to a host with this client, either on the current tokio scope or in a new thread if none is available.
    ///
    /// This shouldn't fail; any errors are because of OS failures to create threads or a runtime.
    pub fn try_spawn<H: Display + Send + 'static>(mut self, host: H) -> std::io::Result<()> {
        let fut = async move {
            let _ = self.connect(host, true).await;
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(fut);
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                std::thread::Builder::new()
                    .name("nt-client-worker".to_string())
                    .spawn(move || rt.block_on(fut))?;
            }
        }
        Ok(())
    }
    /// Connect to a host in the background, panicking on failure.
    ///
    /// See [`try_spawn_client`] for more details.
    pub fn spawn<H: Display + Send + 'static>(self, host: H) {
        self.try_spawn(host).expect("Failed to spawn client");
    }
}

/// A globally accessible handle for a client.
pub static GLOBAL_HANDLE: OnceLock<NtHandle> = OnceLock::new();
