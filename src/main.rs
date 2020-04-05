#![recursion_limit = "512"]

#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

use std::net::SocketAddr;

use error_chain::error_chain;

use structopt::StructOpt;

use hotwatch::{Event, Hotwatch};

use futures_util::future::FutureExt;
use futures_util::select;
use futures_util::sink::SinkExt;

use async_std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    path::PathBuf,
    prelude::*,
    sync::{Arc, Mutex},
    task,
};

use async_tungstenite::tungstenite;

#[derive(Debug, StructOpt)]
pub struct Opts {
    #[structopt(short, long, default_value = "127.0.0.1:53135")]
    bind: String,

    #[structopt(short, long)]
    target: PathBuf,

    #[structopt(short, long)]
    download: bool,

    #[structopt(short, long)]
    play: bool,

    #[structopt(short, long)]
    no_gui: bool,
}

error_chain! {
    foreign_links {
        Io(std::io::Error);
        Hotwatch(hotwatch::Error);
        WebSocket(tungstenite::Error);
        NotificationChannel(std::sync::mpsc::SendError<WorkerNotification>);
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "payload", rename_all = "kebab-case")]
pub enum ServerMessage {
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

pub struct State {
    opts: Arc<Mutex<Opts>>,
}

impl State {
    pub fn new(opts: Arc<Mutex<Opts>>) -> Self {
        Self { opts }
    }
}

async fn handle_accept(
    peer: SocketAddr,
    stream: TcpStream,
    rx_connected: Arc<Mutex<async_std::sync::Receiver<ConnectedMessage>>>,
    tx_conn_notification: async_std::sync::Sender<ConnectedNotification>,
) -> Result<()> {
    let mut ws_stream = async_tungstenite::accept_async(stream).await?;

    info!("accepting connection from {}", peer);

    ws_stream.send(ServerMessage::SendDetails.into()).await?;

    loop {
        let mut rx_ws_lock = rx_connected.lock().await;

        select! {
            msg = ws_stream.next().fuse() => {
                if let Some(msg) = msg {
                    let msg = msg?;
                    debug!("msg: {:?}", msg);

                    if let tungstenite::Message::Text(msg) = msg {
                        let parsed: std::result::Result<ServerMessage, _> = serde_json::from_str(&msg);

                        match parsed {
                            Ok(msg) => match msg {
                                ServerMessage::Details { title, question_id } => {
                                    tx_conn_notification.send(ConnectedNotification::Details { title, question_id }).await;
                                }
                                ServerMessage::Code { code } => {
                                    tx_conn_notification.send(ConnectedNotification::Code { code }).await;
                                }
                                other => {
                                    warn!("unexpected message: {:?}", other);
                                    ws_stream.send(ServerMessage::Error { message: format!("unexpected message") }.into()).await?;
                                }
                            },
                            Err(err) => {
                                error!("failed to parse message: {}", err);
                                ws_stream.send(ServerMessage::Error { message: err.to_string() }.into()).await?;
                            }
                        }
                    }
                } else {
                    break;
                }
            }

            msg = rx_ws_lock.next().fuse() => {
                drop(rx_ws_lock);

                if let Some(msg) = msg {
                    match msg {
                        ConnectedMessage::AppReady => {
                            ws_stream.send(ServerMessage::AppReady.into()).await?;
                        }
                        ConnectedMessage::UpdateCode { code, play } => {
                            ws_stream.send(ServerMessage::UpdateCode { code, play }.into()).await?;
                        }
                        ConnectedMessage::SendCode => {
                            ws_stream.send(ServerMessage::SendCode.into()).await?;
                        }
                        ConnectedMessage::Terminate => { break; }
                    }
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn accept_connection(
    peer: SocketAddr,
    stream: TcpStream,
    rx_connected: Arc<Mutex<async_std::sync::Receiver<ConnectedMessage>>>,
    tx_conn_notification: async_std::sync::Sender<ConnectedNotification>,
) -> Result<()> {
    if let Err(e) = handle_accept(peer, stream, rx_connected, tx_conn_notification).await {
        match e {
            Error(ErrorKind::WebSocket(tungstenite::Error::ConnectionClosed), _)
            | Error(ErrorKind::WebSocket(tungstenite::Error::Protocol(_)), _)
            | Error(ErrorKind::WebSocket(tungstenite::Error::Utf8), _) => (),
            err => error!("error processing connection: {:?}", err),
        }
    }

    Ok(())
}

async fn handle_deny(peer: SocketAddr, stream: TcpStream) -> Result<()> {
    let mut ws_stream = async_tungstenite::accept_async(stream).await?;

    info!("denying connection from {}", peer);
    ws_stream
        .send(ServerMessage::AlreadyConnected.into())
        .await?;

    ws_stream.close(None).await?;

    Ok(())
}

async fn deny_connection(peer: SocketAddr, stream: TcpStream) -> Result<()> {
    if let Err(e) = handle_deny(peer, stream).await {
        match e {
            Error(ErrorKind::WebSocket(tungstenite::Error::ConnectionClosed), _)
            | Error(ErrorKind::WebSocket(tungstenite::Error::Protocol(_)), _)
            | Error(ErrorKind::WebSocket(tungstenite::Error::Utf8), _) => (),
            err => error!("error processing connection: {:?}", err),
        }
    }

    Ok(())
}

#[derive(Debug)]
pub enum ConnectedMessage {
    AppReady,
    UpdateCode { code: String, play: bool },
    SendCode,
    Terminate,
}

#[derive(Debug)]
pub enum WorkerMessage {
    FileChanged { code: String },
    WatchError { error: std::io::Error },
    Start { download: bool },
    Stop,
    Terminate,
}

#[derive(Debug)]
pub enum WorkerNotification {
    Details { title: String, question_id: i32 },
    Initialized,
    Stopped,
    Terminate,
}

#[derive(Debug)]
pub enum ConnectedNotification {
    Details { title: String, question_id: i32 },
    Code { code: String },
}

#[derive(Debug)]
pub enum ListenMessage {
    Terminate,
}

async fn run_accept(
    rx_connected: async_std::sync::Receiver<ConnectedMessage>,
    mut rx_listen: async_std::sync::Receiver<ListenMessage>,
    tx_conn_notification: async_std::sync::Sender<ConnectedNotification>,
    addr: impl ToSocketAddrs + std::fmt::Display,
) -> Result<()> {
    let listener = TcpListener::bind(&addr).await?;
    info!("listening on {}", addr);

    let res = semaphore::Semaphore::new(1, ());
    let rx_connected = Arc::new(Mutex::new(rx_connected));

    loop {
        select! {
            accepted = listener.accept().fuse() => {
                if let Ok((stream, _)) = accepted {
                    let peer = stream.peer_addr()?;

                    match res.try_access() {
                        Ok(_) => {
                            task::spawn(accept_connection(
                                peer,
                                stream,
                                rx_connected.clone(),
                                tx_conn_notification.clone(),
                            ));
                        }
                        Err(semaphore::TryAccessError::NoCapacity) => {
                            task::spawn(deny_connection(peer, stream));
                        }
                        Err(_) => break,
                    }
                } else {
                    break;
                }
            },

            terminated = rx_listen.next().fuse() => {
                match terminated {
                    None | Some(ListenMessage::Terminate) => { break; }
                }
            }
        }
    }

    Ok(())
}

async fn run_controller(
    state: State,
    tx_connected: async_std::sync::Sender<ConnectedMessage>,
    tx_listen: async_std::sync::Sender<ListenMessage>,
    mut rx_controller: async_std::sync::Receiver<WorkerMessage>,
    tx_notification: std::sync::mpsc::Sender<WorkerNotification>,
    mut rx_conn_notification: async_std::sync::Receiver<ConnectedNotification>,
) -> Result<()> {
    let mut send_code_pending = false;

    loop {
        select! {
            msg = rx_controller.next().fuse() => {
                trace!("msg: {:?}", msg);

                if let Some(msg) = msg {
                    match msg {
                        WorkerMessage::FileChanged { code } => {
                            trace!("controller: file changed");

                            tx_connected.send(ConnectedMessage::UpdateCode { code, play: state.opts.lock().await.play }).await;

                            trace!("controller: file changed end");
                        }
                        WorkerMessage::WatchError { error } => {
                            warn!("file watcher error: {}", error);
                        }
                        WorkerMessage::Start { download } => {
                            trace!("controller: start");

                            // Update local file if download was requested
                            send_code_pending = download;

                            // We are now ready
                            tx_connected.send(ConnectedMessage::AppReady).await;

                            // Notify UI
                            tx_notification.send(WorkerNotification::Initialized)?;

                            trace!("controller: start end");
                        }
                        WorkerMessage::Stop => {
                            trace!("controller: stop");

                            // Discard any notifications from IDE
                            send_code_pending = false;

                            // Notify UI
                            tx_notification.send(WorkerNotification::Stopped)?;

                            trace!("controller: stop end");
                        }
                        WorkerMessage::Terminate => {
                            break;
                        }
                    }
                } else {
                    break;
                }
            },

            msg = rx_conn_notification.next().fuse() => {
                if let Some(msg) = msg {
                    match msg {
                        ConnectedNotification::Details { title, question_id } => {
                            trace!("controller: details");

                            // Notify the UI we now have a question
                            tx_notification.send(WorkerNotification::Details { title: title.clone(), question_id })?;

                            trace!("controller: details end");

                        },
                        ConnectedNotification::Code { code } => {
                            trace!("controller: code");

                            if send_code_pending {
                                match std::fs::write(&state.opts.lock().await.target, code) {
                                    Ok(_) => info!("updated code from IDE"),
                                    Err(err) => {
                                        let message = err.to_string();
                                        error!("{}", message);
                                    }
                                }

                                send_code_pending = false;
                            }

                            trace!("controller: code end");
                        }
                    }
                }
            }
        }
    }

    info!("controller terminating");

    // Terminate connected
    tx_connected.send(ConnectedMessage::Terminate).await;

    // Terminate listener
    tx_listen.send(ListenMessage::Terminate).await;

    // Terminate notification
    tx_notification.send(WorkerNotification::Terminate)?;

    Ok(())
}

fn spawn_worker(
    opts: Arc<Mutex<Opts>>,
) -> Result<(
    std::thread::JoinHandle<Result<()>>,
    async_std::sync::Sender<WorkerMessage>,
    std::sync::mpsc::Receiver<WorkerNotification>,
)> {
    let state = State::new(opts.clone());

    let mut hotwatch = Hotwatch::new()?;
    let path: PathBuf =
        task::block_on(async { opts.lock().await.target.parent().unwrap().to_owned() });

    let (tx_controller, rx_controller) = async_std::sync::channel(1);
    let (tx_listen, rx_listen) = async_std::sync::channel(1);
    let (tx_connected, rx_connected) = async_std::sync::channel(1);
    let (tx_notification, rx_notification) = std::sync::mpsc::channel();
    let (tx_conn_notification, rx_conn_notification) = async_std::sync::channel(1);

    {
        let opts = opts.clone();
        let tx_controller = tx_controller.clone();

        hotwatch.watch(path, move |event: Event| match event {
            Event::NoticeWrite(path) | Event::Create(path) | Event::Write(path) => {
                let tx_controller = tx_controller.clone();
                let opts = opts.clone();

                task::spawn(async move {
                    if let Ok(target) = async_std::fs::canonicalize(&opts.lock().await.target).await
                    {
                        if PathBuf::from(path) == target {
                            match async_std::fs::read_to_string(&target).await {
                                Ok(code) => {
                                    tx_controller
                                        .send(WorkerMessage::FileChanged { code })
                                        .await;
                                }
                                Err(error) => {
                                    tx_controller
                                        .send(WorkerMessage::WatchError { error })
                                        .await;
                                }
                            }
                        }
                    }
                });
            }
            _ => {}
        })?;
    }

    Ok((
        std::thread::spawn(move || {
            task::block_on(async move {
                task::spawn(run_accept(
                    rx_connected,
                    rx_listen,
                    tx_conn_notification,
                    opts.lock().await.bind.clone(),
                ));

                run_controller(
                    state,
                    tx_connected,
                    tx_listen,
                    rx_controller,
                    tx_notification,
                    rx_conn_notification,
                )
                .await
            })?;

            // Stop watching when the async worker completes
            drop(hotwatch);

            Ok(())
        }),
        tx_controller,
        rx_notification,
    ))
}

#[paw::main]
fn main(opts: Opts) -> Result<()> {
    let no_gui = opts.no_gui;
    if no_gui {
        env_logger::init_from_env(
            env_logger::Env::new()
                .filter_or("CG_LOCAL_LOG", "cg_local_app=debug")
                .write_style("CG_LOCAL_LOG_STYLE"),
        );
    }

    let opts = Arc::new(Mutex::new(opts));
    let (join_handle, tx_worker, rx_notification) = spawn_worker(opts.clone())?;

    if no_gui {
        for m in rx_notification.iter() {
            match m {
                WorkerNotification::Details { title, question_id } => {
                    info!("working on question '{}' (id: {})", title, question_id);

                    task::block_on(async {
                        trace!("sending Start");

                        tx_worker
                            .send(WorkerMessage::Start {
                                download: opts.lock().await.download,
                            })
                            .await
                    });
                }
                WorkerNotification::Initialized => {
                    info!("synchronization started");
                }
                WorkerNotification::Stopped => {
                    info!("synchronization stopped");
                }
                WorkerNotification::Terminate => {
                    break;
                }
            }
        }
    } else {
        use cursive::views::{Checkbox, Dialog, LinearLayout, TextView};
        use cursive::Cursive;

        fn dialog_waiting(s: &mut Cursive) {
            s.pop_layer();
            s.add_layer(
                Dialog::around(TextView::new("Waiting for IDE to connect."))
                    .title("cg-local-app.rs")
                    .button("Quit", |s| s.quit()),
            );
        }

        fn dialog_initial(
            s: &mut Cursive,
            header: &str,
            tx_worker: async_std::sync::Sender<WorkerMessage>,
        ) {
            s.pop_layer();
            s.add_layer(
                Dialog::around(TextView::new(header))
                    .title("cg-local-app.rs")
                    .button("Upload", {
                        let tx_worker = tx_worker.clone();
                        move |_| {
                            task::block_on(
                                tx_worker.send(WorkerMessage::Start { download: false }),
                            );
                        }
                    })
                    .button("Download", move |_| {
                        task::block_on(tx_worker.send(WorkerMessage::Start { download: true }));
                    })
                    .button("Quit", |s| s.quit()),
            );
        }

        fn dialog_running(
            s: &mut Cursive,
            header: &str,
            tx_worker: async_std::sync::Sender<WorkerMessage>,
            opts: Arc<Mutex<Opts>>,
        ) {
            s.pop_layer();
            s.add_layer(
                Dialog::around(
                    LinearLayout::vertical().child(TextView::new(header)).child(
                        LinearLayout::horizontal()
                            .child({
                                let mut chk = Checkbox::new().on_change({
                                    let opts = opts.clone();
                                    move |_s, checked| {
                                        task::block_on(async { opts.lock().await.play = checked });
                                    }
                                });

                                if task::block_on(async { opts.lock().await.play }) {
                                    chk.check();
                                }

                                chk
                            })
                            .child(TextView::new("Play on upload")),
                    ),
                )
                .title("cg-local-app.rs")
                .button("Stop sync", move |_| {
                    task::block_on(tx_worker.send(WorkerMessage::Stop));
                })
                .button("Quit", |s| s.quit()),
            );
        }

        let mut s = Cursive::default();
        s.add_global_callback('q', |s| s.quit());

        dialog_waiting(&mut s);

        s.refresh();

        let mut header = String::new();

        loop {
            s.step();
            if !s.is_running() {
                break;
            }

            let mut needs_refresh = false;
            for m in rx_notification.try_iter() {
                match m {
                    WorkerNotification::Details { title, question_id } => {
                        header = format!("Working on question '{}' (id: {})", title, question_id);

                        dialog_initial(&mut s, &header, tx_worker.clone());
                    }
                    WorkerNotification::Initialized => {
                        // Show running screen
                        dialog_running(&mut s, &header, tx_worker.clone(), opts.clone());
                    }
                    WorkerNotification::Stopped => {
                        // Go back to question screen
                        dialog_initial(&mut s, &header, tx_worker.clone());
                    }
                    WorkerNotification::Terminate => {
                        s.quit();
                    }
                }

                needs_refresh = true;
            }

            if needs_refresh {
                s.refresh();
            }
        }
    }

    // Terminate worker
    task::block_on(tx_worker.send(WorkerMessage::Terminate));
    join_handle.join().unwrap()?;

    Ok(())
}
