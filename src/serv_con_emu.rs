use std::{
    collections::VecDeque,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
};

use audiopus::SampleRate;

use crate::{ServCon, SpeakerMsg, MicMsg};

pub struct ServEmu {}

impl ServEmu {
    pub fn new() -> Self {
        Self {}
    }
}

const OPUS_BUF_SIZE: usize = 960;

impl ServCon for ServEmu {
    fn run(self, tx: Sender<SpeakerMsg>, rx: Receiver<MicMsg>) -> JoinHandle<()> {
        std::thread::Builder::new()
            .name("ServCon".into())
            .spawn(move || {
                let encoder = audiopus::coder::Encoder::new(
                    SampleRate::Hz48000,
                    audiopus::Channels::Mono,
                    audiopus::Application::Voip,
                )
                .expect("Can't build Opus encoder");
                let mut decoder =
                    audiopus::coder::Decoder::new(SampleRate::Hz48000, audiopus::Channels::Mono)
                        .expect("Can't build Opus decoder");

                let mut total_mic_buf: VecDeque<f32> = VecDeque::new();
                while let Ok(msg) = rx.recv() {
                    match msg {
                        MicMsg::AudioFromMic(audio_buf) => {
                            total_mic_buf.extend(audio_buf.iter());
                        }
                        MicMsg::Shutdown => break,
                    }

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

                        let mut audio_output: Vec<f32> = vec![0.0; OPUS_BUF_SIZE];
                        let decoded_len = decoder
                            .decode_float(Some(&net_buf[..enc_pkt_len]), &mut audio_output, false)
                            .expect("Can't decode");

                        tx.send(SpeakerMsg::AudioFromSrv(audio_output))
                            .expect("Can't send");
                    }
                }
            })
            .expect("Can't spawn ServCon thread")
    }
}
