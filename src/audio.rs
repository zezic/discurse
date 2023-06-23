use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, Sample, SampleFormat, Stream};
use log::{info, warn};

use crate::{MicMsg, SpeakerMsg};

pub fn audio_worker(
    stx: Sender<MicMsg>,
    crx: Receiver<SpeakerMsg>,
    shutdown_rx: Receiver<()>,
) -> Result<()> {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("No output device available");

    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("Error while querying configs");
    let supported_config = supported_configs_range
        .next()
        .expect("No supported config?!")
        .with_max_sample_rate();

    let sample_format = supported_config.sample_format();

    let config = device.default_output_config().unwrap();
    println!("Default output config: {:?}", config);
    let writer = match sample_format {
        cpal::SampleFormat::F32 => audio_writer::<f32>(&device, &config.clone().into(), crx),
        cpal::SampleFormat::I16 => audio_writer::<i16>(&device, &config.clone().into(), crx),
        cpal::SampleFormat::U16 => audio_writer::<u16>(&device, &config.clone().into(), crx),
    }
    .expect("Can't run audio writer");

    let config = device.default_input_config().unwrap();
    println!("Default input config: {:?}", config);
    let reader = match sample_format {
        cpal::SampleFormat::F32 => audio_reader::<f32>(&device, &config.into(), stx),
        cpal::SampleFormat::I16 => audio_reader::<i16>(&device, &config.into(), stx),
        cpal::SampleFormat::U16 => audio_reader::<u16>(&device, &config.into(), stx),
    }
    .expect("Can't run audio reader");

    shutdown_rx.recv().expect("Can't receive shutdown msg");

    Ok(())
}

fn audio_writer<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    rx: Receiver<SpeakerMsg>,
) -> Result<Stream, anyhow::Error>
where
    T: 'static + cpal::Sample,
{
    let channels = config.channels as usize;
    let mut ringbuf: VecDeque<f32> = VecDeque::new();
    let write_callback = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        while ringbuf.len() < data.len() / channels {
            let mic_buffer = rx.recv().expect("Can't receive from channel");
            match mic_buffer {
                SpeakerMsg::AudioFromSrv(mic_buffer) => {
                    ringbuf.extend(mic_buffer.iter());
                }
            }
        }
        for frame in data.chunks_mut(channels) {
            let from_mic = ringbuf.pop_front().expect("Not enough smp in ringbuf");
            for smp in frame {
                *smp = Sample::from(&from_mic);
            }
        }
    };
    let err_fn = |err| eprintln!("An error occurred on the output audio stream: {}", err);
    let stream = device.build_output_stream::<T, _, _>(config, write_callback, err_fn)?;
    stream.play().unwrap();
    info!("Audio output stream has been started");
    Ok(stream)
}

fn audio_reader<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender<MicMsg>,
) -> Result<Stream, anyhow::Error>
where
    T: 'static + cpal::Sample,
{
    let channels = config.channels as usize;
    let read_callback = move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mut mic_buffer = vec![];
        for frame in data.chunks(channels) {
            // Mixdown to mono and amplify by 4.0
            let sum = frame.iter().map(|smp| smp.to_f32()).sum::<f32>() * 4.0;
            mic_buffer.push(sum);
        }
        tx.send(MicMsg::AudioFromMic(mic_buffer))
            .expect("Can't send mic data over channel");
    };
    let err_fn = |err| eprintln!("An error occurred on the output audio stream: {}", err);
    let stream = device.build_input_stream::<T, _, _>(config, read_callback, err_fn)?;
    stream.play().unwrap();
    info!("Audio input stream has been started");
    Ok(stream)
}
