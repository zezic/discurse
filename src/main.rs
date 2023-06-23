use std::env;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use anyhow::Result;
use fast_log::Config;
// use serv_con_emu::ServEmu;
use serv_con_real::ServReal;

mod audio;
mod serv_con_emu;
mod serv_con_real;

pub enum MicMsg {
    AudioFromMic(Vec<f32>),
    Shutdown,
}

pub enum SpeakerMsg {
    AudioFromSrv(Vec<f32>),
}

pub trait ServCon {
    fn run(self, tx: Sender<SpeakerMsg>, rx: Receiver<MicMsg>) -> JoinHandle<()>;
}

fn main() -> Result<()> {
    fast_log::init(Config::new().console()).expect("Can't initialize logger");

    let addr = env::args().into_iter().nth(1).unwrap_or(String::from("zezic.ru:13337"));

    let (ctx, crx) = std::sync::mpsc::channel();
    let (stx, srx) = std::sync::mpsc::channel();

    let shutdown_stx = stx.clone();

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();

    // let mut serv = ServEmu::new();
    let mut serv = ServReal::new(addr);
    let serv_handle = serv.run(ctx, srx);

    let audio_thread = std::thread::Builder::new()
        .name("Audio".into())
        .spawn(move || audio::audio_worker(stx, crx, shutdown_rx))?;

    ctrlc::set_handler(move || {
        shutdown_tx.send(()).expect("Can't send shutdown");
        shutdown_stx.send(MicMsg::Shutdown).expect("Can't send to serv emu");
    }).expect("Can't set Ctrl-C handler");

    audio_thread.join().expect("Can't join audio thread")?;
    serv_handle.join().expect("Can't join serv handle");

    log::logger().flush();
    Ok(())
}
