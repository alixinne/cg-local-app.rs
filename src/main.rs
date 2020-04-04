#![recursion_limit = "512"]

#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

use std::net::SocketAddr;
use std::path::PathBuf;

use structopt::StructOpt;

use hotwatch::{Event, Hotwatch};

use futures_util::future::FutureExt;
use futures_util::select;
use futures_util::sink::SinkExt;

use async_std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    prelude::*,
    task,
};

use async_tungstenite::tungstenite;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(short, long, default_value = "127.0.0.1:53135")]
    bind: String,

    #[structopt(short, long)]
    target: PathBuf,

    #[structopt(short, long)]
    from_website: bool,

    #[structopt(short, long)]
    play: bool,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "payload", rename_all = "kebab-case")]
enum ServerMessage {
    SendDetails,
    #[serde(rename_all = "camelCase")]
    Details {
        title: String,
        question_id: i32,
    },
    AppReady,
    AlreadyConnected,
    UpdateCode {
        code: String,
        play: bool,
    },
    SendCode,
    Code {
        code: String,
    },
    SetReadOnly {
        state: bool,
    },
    Error {
        message: String,
    },
}

impl Into<tungstenite::Message> for ServerMessage {
    fn into(self) -> tungstenite::Message {
        tungstenite::Message::Text(serde_json::to_string(&self).unwrap())
    }
}

use async_std::sync::{Arc, Mutex};
type StateHandle = Arc<Mutex<State>>;

struct State {
    target: PathBuf,
    download_at_start: bool,
}

impl State {
    pub fn new(opts: &Opts) -> Self {
        Self {
            target: opts.target.clone(),
            download_at_start: opts.from_website,
        }
    }
}

async fn handle_accept(
    peer: SocketAddr,
    stream: TcpStream,
    state: StateHandle,
    rx_ws: Arc<Mutex<async_std::sync::Receiver<ServerMessage>>>,
) -> tungstenite::Result<()> {
    let mut ws_stream = async_tungstenite::accept_async(stream).await?;

    info!("accepting connection from {}", peer);

    ws_stream.send(ServerMessage::SendDetails.into()).await?;

    loop {
        let mut rx_ws_lock = rx_ws.lock().await;

        select! {
            msg = ws_stream.next().fuse() => {
                if let Some(msg) = msg {
                    let msg = msg?;
                    debug!("msg: {:?}", msg);

                    if let tungstenite::Message::Text(msg) = msg {
                        let parsed: ServerMessage = serde_json::from_str(&msg).expect("failed to parse object");

                        match parsed {
                            ServerMessage::Details { title, question_id } => {
                                info!("working on {} (id {})", title, question_id);

                                // TODO: "Initialize controller"
                                ws_stream.send(ServerMessage::AppReady.into()).await?;

                                if state.lock().await.download_at_start {
                                    ws_stream.send(ServerMessage::SendCode.into()).await?;
                                }
                            }
                            ServerMessage::Code { code } => {
                                match std::fs::write(&state.lock().await.target, code) {
                                    Ok(_) => info!("updated code from IDE"),
                                    Err(err) => {
                                        let message = err.to_string();
                                        error!("{}", message);
                                        ws_stream
                                            .send(ServerMessage::Error { message }.into())
                                            .await?
                                    }
                                }
                            }
                            other => debug!("parsed: {:?}", other),
                        }
                    }
                } else {
                    break;
                }
            }

            msg = rx_ws_lock.next().fuse() => {
                drop(rx_ws_lock);

                if let Some(msg) = msg {
                    ws_stream.send(msg.into()).await?;
                }
            }
        }
    }

    Ok(())
}

async fn accept_connection(
    peer: SocketAddr,
    stream: TcpStream,
    state: StateHandle,
    rx_ws: Arc<Mutex<async_std::sync::Receiver<ServerMessage>>>,
) {
    use tungstenite::Error;

    let result = handle_accept(peer, stream, state, rx_ws).await;
    info!("connection from {} terminated", peer);

    match result {
        Ok(_) | Err(Error::ConnectionClosed) | Err(Error::Protocol(_)) | Err(Error::Utf8) => (),
        err => error!("error processing connection: {:?}", err),
    }
}

async fn handle_deny(peer: SocketAddr, stream: TcpStream) -> tungstenite::Result<()> {
    let mut ws_stream = async_tungstenite::accept_async(stream).await?;

    info!("denying connection from {}", peer);
    ws_stream
        .send(ServerMessage::AlreadyConnected.into())
        .await?;

    ws_stream.close(None).await?;

    Ok(())
}

async fn deny_connection(peer: SocketAddr, stream: TcpStream) {
    use tungstenite::Error;

    match handle_deny(peer, stream).await {
        Ok(_) | Err(Error::ConnectionClosed) | Err(Error::Protocol(_)) | Err(Error::Utf8) => (),
        err => error!("error processing connection: {:?}", err),
    }
}

async fn run_loop(
    state: State,
    rx_ws: Arc<Mutex<async_std::sync::Receiver<ServerMessage>>>,
    addr: impl ToSocketAddrs + std::fmt::Display,
) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    info!("listening on {}", addr);

    let res = semaphore::Semaphore::new(1, ());
    let state = Arc::new(Mutex::new(state));

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr()?;
        info!("peer address: {}", peer);

        match res.try_access() {
            Ok(_) => {
                task::spawn(accept_connection(
                    peer,
                    stream,
                    state.clone(),
                    rx_ws.clone(),
                ));
            }
            Err(semaphore::TryAccessError::NoCapacity) => {
                task::spawn(deny_connection(peer, stream));
            }
            Err(_) => break,
        }
    }

    Ok(())
}

#[paw::main]
fn main(opts: Opts) -> Result<()> {
    env_logger::init();

    let (tx_ws, rx_ws) = async_std::sync::channel(1);

    let state = State::new(&opts);

    let mut hotwatch = Hotwatch::new()?;

    let target = opts.target.clone();
    let play = opts.play;

    hotwatch.watch(state.target.parent().unwrap(), move |event: Event| {
        debug!("hotwatch: {:?}", event);

        match event {
            Event::NoticeWrite(path) | Event::Create(path) | Event::Write(path) => {
                if let Ok(target) = std::fs::canonicalize(&target) {
                    if path == target {
                        match std::fs::read_to_string(&target) {
                            Ok(code) => {
                                info!("uploading code to IDE");

                                task::block_on(
                                    tx_ws.send(ServerMessage::UpdateCode { code, play }),
                                );
                            }
                            Err(error) => {
                                task::block_on(tx_ws.send(ServerMessage::Error {
                                    message: error.to_string(),
                                }));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    })?;

    task::block_on(run_loop(state, Arc::new(Mutex::new(rx_ws)), opts.bind))
}
