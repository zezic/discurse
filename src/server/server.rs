use std::{
    collections::HashMap,
    net::{Shutdown, TcpListener, TcpStream},
    sync::mpsc::{self, Receiver, Sender},
};

use anyhow::Result;
use fast_log::Config;
use log::{info, warn};
use uuid::Uuid;

use discurse::protocol::{ClientMsg, ServerMsg, write_msg, FromMsg, Gone, socket_reader};

const PROTOCOL_VERSION: u64 = 1;

fn client_writer(mut stream: TcpStream, cwrx: Receiver<ToClient>) {
    let msg = ServerMsg::Version(PROTOCOL_VERSION);
    write_msg(&mut stream, msg);

    while let Ok(msg) = cwrx.recv() {
        match msg {
            ToClient::Audio(id, audio) => {
                let msg = ServerMsg::OpusAudio(id.into(), audio);
                write_msg(&mut stream, msg);
            }
            ToClient::Shutdown => {
                let msg = ServerMsg::Bye {
                    reason: String::from("Server requested shutdown"),
                };
                write_msg(&mut stream, msg);
                stream
                    .shutdown(Shutdown::Both)
                    .expect("Can't shutdown stream");
            }
        }
    }
}

enum ToBroadcaster {
    NewClient(Uuid, Sender<ToClient>),
    NewPacket(Uuid, ClientMsg),
    ClientGone(Uuid),
}

impl FromMsg<ClientMsg> for ToBroadcaster {
    fn from_msg(client_id: Option<Uuid>, msg: ClientMsg) -> Self {
        Self::NewPacket(client_id.expect("Expected client id"), msg)
    }
}

impl Gone for ToBroadcaster {
    fn gone(client_id: Option<Uuid>) -> Self {
        Self::ClientGone(client_id.expect("Expected client id"))
    }
}

enum ToClient {
    Audio(Uuid, Vec<u8>),
    Shutdown,
}

struct Client {
    nickname: Option<String>,
    tx: Sender<ToClient>,
}

fn broadcaster(rx: Receiver<ToBroadcaster>) {
    let mut clients = HashMap::new();

    while let Ok(msg) = rx.recv() {
        match msg {
            ToBroadcaster::NewClient(id, ctx) => {
                clients.insert(
                    id,
                    Client {
                        nickname: None,
                        tx: ctx,
                    },
                );
            }
            ToBroadcaster::NewPacket(id, packet) => match packet {
                ClientMsg::GetClients => todo!(),
                ClientMsg::Nickname(nickname) => {
                    let mut client = clients.get_mut(&id).expect("No client");
                    client.nickname = Some(nickname);
                }
                ClientMsg::OpusAudio(audio) => {
                    clients.retain(|&recv_id, client| {
                        if recv_id == id {
                            return true;
                        }
                        match client.tx.send(ToClient::Audio(id, audio.clone())) {
                            Ok(()) => {
                                true
                            },
                            Err(err) => {
                                warn!("Can't notify client {}, dropping: {}", recv_id, err);
                                false
                            }
                        }
                    });
                }
                ClientMsg::Leave => {
                    clients.remove(&id);
                }
            },
            ToBroadcaster::ClientGone(id) => {
                clients.remove(&id);
            }
        };
    }

    info!("Broadcaster exits");
}

fn main() -> Result<()> {
    fast_log::init(Config::new().console()).expect("Can't initialize logger");
    let listener = TcpListener::bind("0.0.0.0:13337").expect("Can't bind to port 13337");

    let (btx, brx) = mpsc::channel();

    let broadcaster_handle = std::thread::Builder::new()
        .name("Broadcaster".into())
        .spawn(move || {
            broadcaster(brx);
        })
        .expect("Can't start broadcaster");

    while let Ok((stream, addr)) = listener.accept() {
        let id = uuid::Uuid::new_v4();
        info!("Handling client {} x {}", addr, id);

        let crtx = btx.clone();
        let (cwtx, cwrx) = mpsc::channel();

        let stream_read = stream.try_clone().expect("Can't clone stream");
        std::thread::spawn(move || socket_reader::<_, ClientMsg>(stream_read, Some(id), crtx));
        std::thread::spawn(move || client_writer(stream, cwrx));

        btx.send(ToBroadcaster::NewClient(id, cwtx))
            .expect("Can't send to broadcaster");
    }

    broadcaster_handle
        .join()
        .expect("Can't join broadcaster handle");

    log::logger().flush();

    Ok(())
}
