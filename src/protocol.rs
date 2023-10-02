use std::collections::VecDeque;


#[derive(Debug)]
pub struct DecodeError {}


impl std::error::Error for DecodeError {
    fn description(&self) -> &str {
        "Bad WS frame received from a client!"
    }
}


impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Protocol Decode Error")
    }
}


pub trait ProtocolFrame : Sized {
    fn encode(&self) -> Vec<u8>;
    fn decode(data : VecDeque<u8>) -> Result<Self, DecodeError>;
    fn manifest() -> &'static str; // manifest of this protocol frame type.
}

pub trait ProtocolSegment : Sized {
    fn encode(self) -> Vec<u8>;
    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError>;
}

impl ProtocolSegment for u8 {
    fn encode(self) -> Vec<u8> {
        let mut v = Vec::with_capacity(1);
        v.push(self);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        data.pop_front().ok_or(DecodeError {})
    }
}


impl ProtocolSegment for bool {
    fn encode(self) -> Vec<u8> {
        let mut v = Vec::with_capacity(1);
        v.push(if self { 1 } else { 0 });
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        Ok(if data.pop_front().ok_or(DecodeError {})? == 1 { true } else { false })
    }
}

impl ProtocolSegment for u16 {
    fn encode(self) -> Vec<u8> {
        let mut v : Vec<u8> = Vec::with_capacity(2);
        let bytes = self.to_be_bytes();
        v.push(bytes[0]); // flip the byte order so it pops in properly
        v.push(bytes[1]);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let r = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        Ok(Self::from_be_bytes(r))
    }
}


impl ProtocolSegment for u32 {
    fn encode(self) -> Vec<u8> {
        let mut v : Vec<u8> = Vec::with_capacity(4);
        let bytes = self.to_be_bytes();
        v.push(bytes[0]); // flip the byte order so it pops in properly
        v.push(bytes[1]);
        v.push(bytes[2]);
        v.push(bytes[3]);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let r = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        Ok(Self::from_be_bytes(r))
    }
}


impl ProtocolSegment for u64 {
    fn encode(self) -> Vec<u8> {
        let mut v : Vec<u8> = Vec::with_capacity(8);
        let bytes = self.to_be_bytes();
        v.push(bytes[0]);
        v.push(bytes[1]);
        v.push(bytes[2]);
        v.push(bytes[3]);
        v.push(bytes[4]);
        v.push(bytes[5]);
        v.push(bytes[6]);
        v.push(bytes[7]);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let r = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        Ok(Self::from_be_bytes(r))
    }
}


impl ProtocolSegment for i32 {
    fn encode(self) -> Vec<u8> {
        let mut v : Vec<u8> = Vec::with_capacity(4);
        let bytes = self.to_be_bytes();
        v.push(bytes[0]); // flip the byte order so it pops in properly
        v.push(bytes[1]);
        v.push(bytes[2]);
        v.push(bytes[3]);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let r = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        Ok(Self::from_be_bytes(r))
    }
}



impl ProtocolSegment for f32 {
    fn encode(self) -> Vec<u8> {
        let mut v : Vec<u8> = Vec::with_capacity(4);
        let bytes = self.to_be_bytes();
        v.push(bytes[0]); // flip the byte order so it pops in properly
        v.push(bytes[1]);
        v.push(bytes[2]);
        v.push(bytes[3]);
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let r = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        Ok(Self::from_be_bytes(r))
    }
}

impl ProtocolSegment for String {
    fn encode(self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.len() + 2); // space enough for me and my size information
        v.append(&mut Vec::from((self.len() as u16).to_be_bytes()));
        v.append(&mut Vec::from(self.clone().into_bytes()));
        v
    }

    fn decode(data : &mut VecDeque<u8>) -> Result<Self, DecodeError> {
        let len : [u8; 2] = [data.pop_front().ok_or(DecodeError {})?, data.pop_front().ok_or(DecodeError {})?];
        let len = u16::from_be_bytes(len);
        if data.len() >= len.into() {
            let dat = data.drain(0..len.into()).collect();
            match String::from_utf8(dat) {
                Ok(str) => Ok(str),
                Err(_) => {Err(DecodeError{})}
            }
        }
        else {
            Err(DecodeError{})
        }
    }
}

pub fn protocol_encode<T : ProtocolSegment>(e : T) -> Vec<u8> { // enforces the trait bounds
    e.encode()
}

pub fn protocol_decode<T : ProtocolSegment>(d : &mut VecDeque<u8>) -> Result<T, DecodeError> {
    T::decode(d)
}