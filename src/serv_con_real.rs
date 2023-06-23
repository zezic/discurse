use std::{
    collections::VecDeque,
    net::TcpStream,
    sync::mpsc::{Receiver, Sender, self},
    thread::JoinHandle,
};

use audiopus::{SampleRate, Bitrate};
use discurse::protocol::{ServerMsg, FromMsg, Gone, socket_reader, ClientMsg, write_msg};
use log::info;
use uuid::Uuid;

use crate::{MicMsg, ServCon, SpeakerMsg};


enum Incoming {
    NewPacket(ServerMsg),
    ServerGone,
}

impl FromMsg<ServerMsg> for Incoming {
    fn from_msg(client_id: Option<Uuid>, msg: ServerMsg) -> Self {
        Self::NewPacket(msg)
    }
}

impl Gone for Incoming {
    fn gone(client_id: Option<Uuid>) -> Self {
        Self::ServerGone
    }
}

enum Event {
    Incoming(Incoming),
    MicMsg(MicMsg),
}

pub struct ServReal {
    stream: TcpStream,
    tx: Sender<Incoming>,
    rx: Receiver<Incoming>,
}

impl ServReal {
    pub fn new(addr: String) -> Self {
        let stream = TcpStream::connect(addr).expect("Can't connect");
        let (tx, rx) = mpsc::channel();
        let stream_clone = stream.try_clone().expect("Can't clone stream");
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            socket_reader(stream_clone, None, tx_clone)
        });
        Self { stream, tx, rx }
    }
}

// const OPUS_BUF_SIZE: usize = 960;
const OPUS_BUF_SIZE: usize = 2880;

fn serv_redir(srx: Receiver<Incoming>, etx: Sender<Event>) {
    while let Ok(msg) = srx.recv() {
        etx.send(Event::Incoming(msg)).expect("Can't resend srx -> etx");
    }
}

fn mic_redir(srx: Receiver<MicMsg>, etx: Sender<Event>) {
    while let Ok(msg) = srx.recv() {
        etx.send(Event::MicMsg(msg)).expect("Can't resend srx -> etx");
    }
}

impl ServCon for ServReal {
    fn run(mut self, tx: Sender<SpeakerMsg>, rx: Receiver<MicMsg>) -> JoinHandle<()> {
        let (etx, erx) = mpsc::channel();
        let etx_clone = etx.clone();

        std::thread::spawn(|| {
            serv_redir(self.rx, etx_clone)
        });

        std::thread::spawn(|| {
            mic_redir(rx, etx)
        });

        // let quality = audiopus::Application::Voip;
        let quality = audiopus::Application::Voip;
        let samplerate = SampleRate::Hz48000;

        std::thread::Builder::new()
            .name("ServCon".into())
            .spawn(move || {
                let encoder = audiopus::coder::Encoder::new(
                    samplerate,
                    audiopus::Channels::Mono,
                    quality,
                )
                .expect("Can't build Opus encoder");
                let mut decoder =
                    audiopus::coder::Decoder::new(samplerate, audiopus::Channels::Mono)
                        .expect("Can't build Opus decoder");

                let mut total_mic_buf: VecDeque<f32> = VecDeque::new();

                while let Ok(msg) = erx.recv() {
                    match msg {
                        Event::Incoming(inc) => match inc {
                            Incoming::NewPacket(pkt) => match pkt {
                                ServerMsg::Version(version) => {
                                    info!("Server protocol version {}", version);
                                },
                                ServerMsg::Clients(_) => unimplemented!(),
                                ServerMsg::OpusAudio(id, audio) => {
                                    let mut audio_output: Vec<f32> = vec![0.0; OPUS_BUF_SIZE];
                                    let decoded_len = decoder
                                        .decode_float(Some(&audio), &mut audio_output, false)
                                        .expect("Can't decode");

                                    tx.send(SpeakerMsg::AudioFromSrv(audio_output))
                                        .expect("Can't send");
                                },
                                ServerMsg::Bye { reason } => {
                                    info!("Server said bye. Reason: {}", reason);
                                },
                            },
                            Incoming::ServerGone => break,
                        },
                        Event::MicMsg(mic_msg) => {
                            match mic_msg {
                                MicMsg::AudioFromMic(audio_buf) => {
                                    total_mic_buf.extend(audio_buf.iter());
                                }
                                MicMsg::Shutdown => break,
                            };

                            while total_mic_buf.len() >= OPUS_BUF_SIZE {
                                let mut net_buf = vec![0; 1024 * 1024];
                                let mut for_opus = vec![];

                                for _ in 0..OPUS_BUF_SIZE {
                                    let smp = total_mic_buf
                                        .pop_front()
                                        .expect("Not enough in total_mic_buf");
                                    for_opus.push(smp);
                                }

                                let enc_pkt_len = encoder
                                    .encode_float(&for_opus, &mut net_buf)
                                    .expect("Can't encode");

                                let minimal_net_buf = net_buf[0..enc_pkt_len].to_vec();

                                let msg = ClientMsg::OpusAudio(minimal_net_buf);
                                write_msg(&mut self.stream, msg);
                            }
                        }
                    }
                }
            })
            .expect("Can't spawn ServCon thread")
    }
}
