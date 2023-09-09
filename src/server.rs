// This has a single focus: a websocket server. It is not an HTTP server beyond the upgrade mechanism - all non-Upgrade requests will 418 for the memes.
/* 
    Access model:
    create WebSocketServer object with port and address. it's async. call accept on it to get a new client - accept will not return a WebSocketClientStream until
    the handshake is complete. in a different async "thread", call the WebSocketClientStream's get_message function.
    WebSocketServers will be generic - the generic is an implementor of ProtocolFrame, and messages will be dumped into that type. if it errors from poison, the client
    is immediately dropped, no questions asked. Same if the client attempts to broadcast a frame with size more than 65535 bytes. when and only when it's successfully received without
    poison dropping, the get_message function returns with the message.
*/

use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncBufReadExt, BufReader};
use tokio::select;
use crate::protocol::ProtocolFrame;
use tokio::task::JoinSet;
use std::collections::HashMap;
use base64::engine::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};


const PAYLOAD_SIZE_CAP : u64 = 128; // extend to a few mb later, this is very low for testing


pub struct WebSocketServer {
    listener : TcpListener,
    futures  : JoinSet<Option<WebSocketClientStream>>,
    name     : String
}


pub struct WebSocketClientStream {
    rx       : BufReader<OwnedReadHalf>,
    tx       : OwnedWriteHalf,
    pub path : String,
    closed   : bool
}


#[derive(Debug)]
struct BadFrameError{}


impl std::error::Error for BadFrameError {
    fn description(&self) -> &str {
        "Bad WS frame received from a client!"
    }
}


impl std::fmt::Display for BadFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Bad Frame")
    }
}


enum IncomingWebSocketFrame {
    DataFin (Vec<u8>),
    DataUnfin (Vec<u8>),
    Ping,
    Pong,
    Close
}


impl IncomingWebSocketFrame {
    async fn read_in(rx : &mut BufReader<OwnedReadHalf>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut headp1buf : [u8; 2] = [0; 2];
        let mut maskingkeybuf : [u8; 4] = [0; 4];
        rx.read_exact(&mut headp1buf).await?;
        let opcode = headp1buf[0] & 0b00001111;
        let fin = headp1buf[0] & 0b10000000 != 0; // continuation stuff
        if headp1buf[1] & 0b10000000 == 0 { // MASK == 0
            return Err(Box::new(BadFrameError{})); // short circuit: this frame is bad, and probably the client should be dropped.
        }
        let mut payload_len : u64 = (headp1buf[1] & 0b01111111) as u64;
        if payload_len == 126 {
            let mut payload_ext_buf : [u8; 2] = [0; 2];
            rx.read_exact(&mut payload_ext_buf).await?;
            payload_len = u16::from_be_bytes(payload_ext_buf) as u64; // big endian is universally network order, so this should work fine
        }
        else if payload_len == 127 {
            let mut payload_ext_buf : [u8; 8] = [0; 8];
            rx.read_exact(&mut payload_ext_buf).await?;
            payload_len = u64::from_be_bytes(payload_ext_buf);
        }
        rx.read_exact(&mut maskingkeybuf).await?; // it's guaranteed that the next 4 bytes is the masking key because this would have already failed if it weren't.
        if payload_len > PAYLOAD_SIZE_CAP {
            return Err(Box::new(BadFrameError{})); // todo: more specific error stuff
        }
        let mut payloadbuf = vec![0; payload_len as usize];
        rx.read_exact(&mut payloadbuf.as_mut()).await?;
        for i in 0..payloadbuf.len() {
            payloadbuf[i] = payloadbuf[i] ^ maskingkeybuf[i % 4];
        }
        if opcode == 0x9 {
            Ok(Ping)
        }
        else if opcode == 0xA {
            Ok(Pong)
        }
        else if opcode == 0x2 || opcode == 0x0 {
            if fin {
                Ok(DataFin (payloadbuf))
            }
            else {
                Ok(DataUnfin (payloadbuf))
            }
        }
        else if opcode == 0x8 {
            Ok(Close)
        }
        else {
            Err(Box::new(BadFrameError{})) // text ain't supported
        }
    }
}


use IncomingWebSocketFrame::*;


impl WebSocketClientStream {
    pub async fn read<Protocol : ProtocolFrame>(&mut self) -> Option<Protocol> {
        let mut final_data : Vec<u8> = vec![];
        loop {
            let frame = IncomingWebSocketFrame::read_in(&mut self.rx).await.ok()?; // if the reader hits unexpected EOF, this will return None.
            match frame {
                Ping => {}
                Pong => {}
                Close => {
                    self.closed = true;
                    self.send_close().await; // complying websocket clients will close the actual TCP stream after receiving our return close message, so this can be safely ignored - the connection will be dropped all right and proper soon.
                }
                DataFin (mut data) => {
                    final_data.append(&mut data);
                    break;
                }
                DataUnfin (mut data) => {
                    final_data.append(&mut data);
                }
            }
        }
        match ProtocolFrame::decode(final_data.into()) {
            Ok (result) => Some (result),
            Err (_) => {
                println!("Decode error! A client is poisoning!");
                None
            }
        }
    }

    pub async fn send<Protocol : ProtocolFrame>(&mut self, frame : Protocol) -> Result<(), Box<dyn std::error::Error>> {
        let data = frame.encode();
        let ext_len = data.len() > 125;
        let ext_len_2 = data.len() > 65535;
        let mut headerbuf : Vec<u8> = vec![0; if ext_len_2 { 20 } else if ext_len { 4 } else { 2 }];
        headerbuf[0] = 0b10000010; // FIN set, RSV ignored (as they should be), opcode 0x2
        headerbuf[1] = if ext_len_2 { 127 } else if ext_len { 126 } else { data.len() as u8 }; // MASK always unset, this is outgoing
        if ext_len_2 {
            let bytes = (data.len() as u64).to_be_bytes();
            for i in 0..8 {
                headerbuf[2 + i] = bytes[i];
            }
        }
        else if ext_len {
            let bytes = (data.len() as u64).to_be_bytes();
            for i in 0..4 {
                headerbuf[2 + i] = bytes[i];
            }
        }
        self.tx.write(headerbuf.as_slice()).await?;
        self.tx.write(data.as_slice()).await?;
        Ok(())
    }

    async fn send_close(&mut self) {
        let _ = self.tx.write(&[0x8, 0x0]).await; // think about it - if it fails to send, that means the connection is already closed, so we should...
        /****** do nothing ******/
    }

    pub async fn shutdown(&mut self) {
        if !self.closed { // if it's already closed, do nothing.
            self.send_close().await;
            let _ = self.tx.shutdown().await;
            for _ in 0..10 { // read out 10 frames MAX after sending close before leaving; this is just giving the client a chance to handle the close frame if other data is being sent.
                match IncomingWebSocketFrame::read_in(&mut self.rx).await {
                    Err(_) => {
                        break; // the read failed: therefore, the connection must be closed, if not properly.
                    }
                    Ok(Close) => {
                        break;
                    }
                    _ => {} // throw out
                }
            }
        }
    }
}


fn count_up_till<T : PartialEq>(vec : &Vec<T>, thing : T) -> Option<usize> {
    let mut ret : usize = 0;
    while vec[ret] != thing {
        ret += 1;
        if ret == vec.len() {
            return None;
        }
    }
    Some(ret)
}


impl WebSocketServer {
    pub async fn new(port : u16, name : String) -> Self {
        Self {
            listener : TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap(),
            futures  : JoinSet::new(),
            name
        }
    }

    pub async fn accept<InProtocol : 'static + ProtocolFrame, OutProtocol : 'static + ProtocolFrame>(&mut self) -> WebSocketClientStream {
        loop { // todo: handle this in a nicer way (the goal is never to self.futures.join_next() if self.futures is empty, because handling all those Nones can become quite expensive - 100% cpu utilization on at least one core)
            if self.futures.len() > 0 {
                select! {
                    newclient = self.listener.accept() => {
                        match newclient {
                            Ok ((socket, _)) => {
                                self.futures.spawn(Self::handshake::<InProtocol, OutProtocol>(self.name.clone(), socket));
                            },
                            Err (_) => {
                                println!("Socket accept failed. This is not critical.");
                            }
                        }
                    },
                    websocket = self.futures.join_next() => {
                        match websocket {
                            Some (Ok(Some(websocket))) => {
                                return websocket;
                            }
                            _ => {}
                        }
                    }
                }
            }
            else {
                match self.listener.accept().await {
                    Ok ((socket, _)) => {
                        self.futures.spawn(Self::handshake::<InProtocol, OutProtocol>(self.name.clone(), socket));
                    },
                    Err (_) => {
                        println!("Socket accept failed. This is not critical.");
                    }
                }
            }
        }
    }

    async fn upgrade(mut headers : HashMap<String, String>, tx : OwnedWriteHalf, rx : BufReader<OwnedReadHalf>, uri : String) -> Option<WebSocketClientStream> {
        if !headers.contains_key("connection") || !headers.contains_key("upgrade") || !headers["connection"].to_lowercase().contains("upgrade") || headers["upgrade"].to_lowercase() != "websocket" {
            tx.try_write(b"HTTP/1.1 418 I'm A Teapot\r\n\r\nThis server is not equipped for normal HTTP transactions; all it understands is websocket connections. Please set your connection header to upgrade and your upgrade header to websocket. Also set your WebSocket security headers. Thank you.\n").unwrap();
            println!("I'm a TEAPOT, PEOPLE!");
            return None;
        }
        if match headers.get("sec-websocket-version") { Some(v) => { v != "13" } None => true} {
            tx.try_write(b"HTTP/1.1 400 Get It Right, Goddamnit\r\n\r\nTruly this is an achievement, you have managed to fail at setting your websocket version.\nKill yourself.\n").unwrap();
            println!("We have ourselves an idiot.");
            return None;
        }
        if !headers.contains_key("sec-websocket-key") {
            tx.try_write(b"HTTP/1.1 400 I Hate Meddling Kids Like You\r\n\r\nIn fact if you don't have a sec-websocket-key I will be <i>VERY CROSS!</i>. Get fucking BETTER, n00bsh1t!\n").unwrap();
            println!("We have ourselves a really incompetent hacker.");
            return None;
        }
        let keyconcated = headers.remove("sec-websocket-key").unwrap() + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        let shaun = sha1_smol::Sha1::from(keyconcated).hexdigest();
        let shaun_bytes = hex::decode(shaun).unwrap();
        let b64sha1 = BASE64.encode(shaun_bytes);
        tx.try_write(format!("HTTP/1.1 101 Upgrading\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {}\r\n\r\n", b64sha1).as_bytes()).unwrap();
        Some(WebSocketClientStream { rx, tx, path : uri, closed : false })
    }

    async fn handshake<InProtocol : ProtocolFrame, OutProtocol : ProtocolFrame>(name : String, socket : TcpStream) -> Option<WebSocketClientStream> {
        //socket.set_nodelay(true).unwrap(); // this is meant for online games, like MMOSG. Nagle's algorithm will get in the way of proper performance. to compensate for the lack of Nagle, group together messages sanely.
        let (rx, tx) = socket.into_split();
        let mut rxbuf = BufReader::new(rx);
        let mut method = Vec::<u8>::new();
        let mut uri = Vec::<u8>::new();
        let mut version = Vec::<u8>::new();
        rxbuf.read_until(32, &mut method).await.unwrap();
        rxbuf.read_until(32, &mut uri).await.unwrap();
        rxbuf.read_until(10, &mut version).await.unwrap();
        let method = String::from_utf8(method).unwrap().trim().to_string();
        let uri = String::from_utf8(uri).unwrap().trim().to_string();
        let version = String::from_utf8(version).unwrap().trim().to_string();
        println!("Got [{}] request to [{}] with version [{}].", method, uri, version);
        if version != "HTTP/1.1" {
            tx.try_write(b"HTTP/1.1 400 Bad Request\r\n\r\nBad Request: This server is not equipped for http versions besides 1.1.\n").unwrap();
            println!("Bad request header");
            return None;
        }
        
        let mut headers : HashMap<String, String> = HashMap::new();
        let mut headbuf = Vec::<u8>::new();
        loop {
            rxbuf.read_until(10, &mut headbuf).await.unwrap();
            match count_up_till(&headbuf, 32) {
                Some(colon) => {
                    let hname = String::from_utf8(headbuf[0..colon - 1].to_vec()).unwrap().trim().to_string().to_lowercase();
                    let hval = String::from_utf8(headbuf[colon..].to_vec()).unwrap().trim().to_string();
                    headers.insert(hname, hval);
                }
                None => {
                    break;
                }
            }
            headbuf.clear();
        } // case ambiguity for compatibility

        if uri == "/manifest" {
            tx.try_write(format!("HTTP/1.1 200 Everything Is Ight, Cuh\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\n\r\n{{\"application_name\":\"{}\",\"incoming_protocol\":{},\"outgoing_protocol\":{}}}", name, InProtocol::manifest(), OutProtocol::manifest()).as_bytes()).unwrap();
            println!("Client just wanted our manifest.");
            return None; // kill the connection, the client will have to reconnect to get the websocket upgrade. TODO: fix this!
        }
        else {
            Self::upgrade(headers, tx, rxbuf, uri).await
        }
    }
}