use std::{net::TcpStream, io::{Write, Read}, sync::mpsc::Sender};

use borsh::{BorshSerialize, BorshDeserialize, BorshSchema};
use log::warn;
use uuid::Uuid;

const INITIAL_RECV_BUF_SIZE: usize = 256;

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, PartialEq, Debug)]
pub struct UuidWrapper([u8; 16]);

impl From<UuidWrapper> for Uuid {
    fn from(value: UuidWrapper) -> Self {
        Uuid::from_bytes(value.0)
    }
}

impl From<Uuid> for UuidWrapper {
    fn from(value: Uuid) -> Self {
        Self(*value.as_bytes())
    }
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, PartialEq, Debug)]
pub enum ClientMsg {
    GetClients,
    Nickname(String),
    OpusAudio(Vec<u8>),
    Leave,
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, PartialEq, Debug)]
pub enum ServerMsg {
    Version(u64),
    Clients(Vec<ClientDescription>),
    OpusAudio(UuidWrapper, Vec<u8>),
    Bye { reason: String },
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, PartialEq, Debug)]
pub struct ClientDescription {
    nickname: Option<String>,
    uuid: UuidWrapper,
}

pub fn write_msg<T>(stream: &mut TcpStream, msg: T)
where
    T: BorshSerialize + BorshSchema,
{
    let bytes = borsh::try_to_vec_with_schema(&msg).expect("Can't serialize");
    let size = bytes.len() as u32;
    let size_bytes = size.to_le_bytes();
    stream
        .write_all(&size_bytes[0..4])
        .expect("Can't write to stream");
    stream.write_all(&bytes).expect("Can't write to stream");
}

pub trait FromMsg<M> {
    fn from_msg(client_id: Option<Uuid>, msg: M) -> Self;
}

pub trait Gone {
    fn gone(client_id: Option<Uuid>) -> Self;
}

pub fn socket_reader<T, M>(mut stream: TcpStream, peer_id: Option<Uuid>, crtx: Sender<T>)
where
    T: FromMsg<M> + Gone,
    M: BorshDeserialize + BorshSchema,
{
    let mut buf = vec![0; INITIAL_RECV_BUF_SIZE];

    loop {
        let mut size_buf = [0; 4];
        if let Err(err) = stream.read_exact(&mut size_buf) {
            warn!("Can't read msg size from socket: {}", err);
            break;
        }
        let pkt_size = u32::from_le_bytes(size_buf) as usize;
        if buf.len() < pkt_size {
            buf.resize(pkt_size, 0);
        }
        if let Err(err) = stream.read_exact(&mut buf[0..pkt_size]) {
            warn!("Can't read msg body from socket: {}", err);
            break;
        }
        let msg: M = match borsh::try_from_slice_with_schema(&buf[0..pkt_size]) {
            Ok(msg) => msg,
            Err(err) => {
                warn!("Can't parse msg body from socket: {}", err);
                break;
            }
        };
        crtx.send(T::from_msg(peer_id, msg))
            .expect("Can't send ClientMsg to broadcaster");
    }
    crtx.send(T::gone(peer_id))
        .expect("Can't send to broadcaster");
}
