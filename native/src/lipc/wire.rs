//! D-Bus messages, marshalled and unmarshalled. Every alignment counts from
//! the head of the message; a body opens on an 8-byte boundary and aligns the
//! same read on its own.

/// The byte every message this build writes and reads opens with. libdbus
/// marshals in the sender's own order, and every device here is little.
const LITTLE: u8 = b'l';

/// The protocol version every message carries.
const VERSION: u8 = 1;

/// The fixed head: `y y y y u u`, with the header array's own length at 12.
const HEAD: usize = 16;

/// The flag on a call wanting no answer.
const NO_REPLY: u8 = 0x1;

/// The header field codes.
const PATH: u8 = 1;
const INTERFACE: u8 = 2;
const MEMBER: u8 = 3;
const ERROR_NAME: u8 = 4;
const REPLY_SERIAL: u8 = 5;
const DESTINATION: u8 = 6;
const SENDER: u8 = 7;
const SIGNATURE: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    MethodCall,
    MethodReturn,
    Error,
    Signal,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Kind::MethodCall => 1,
            Kind::MethodReturn => 2,
            Kind::Error => 3,
            Kind::Signal => 4,
        }
    }

    fn of_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Kind::MethodCall),
            2 => Some(Kind::MethodReturn),
            3 => Some(Kind::Error),
            4 => Some(Kind::Signal),
            _ => None,
        }
    }
}

/// One message, its body held as the bytes it arrived in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub kind: Option<Kind>,
    pub serial: u32,
    pub flags: u8,
    pub path: String,
    pub interface: String,
    pub member: String,
    pub error: String,
    pub reply_serial: u32,
    pub destination: String,
    pub sender: String,
    pub signature: String,
    pub body: Vec<u8>,
}

impl Message {
    pub fn call(destination: &str, path: &str, interface: &str, member: &str) -> Self {
        Self {
            kind: Some(Kind::MethodCall),
            destination: destination.into(),
            path: path.into(),
            interface: interface.into(),
            member: member.into(),
            ..Self::default()
        }
    }

    /// An answer to `call`, carrying `body` under `signature`.
    pub fn answer(call: &Message, signature: &str, body: Vec<u8>) -> Self {
        Self {
            kind: Some(Kind::MethodReturn),
            reply_serial: call.serial,
            destination: call.sender.clone(),
            signature: signature.into(),
            body,
            ..Self::default()
        }
    }

    /// A refusal of `call`, named by `error`.
    pub fn refuse(call: &Message, error: &str) -> Self {
        Self {
            kind: Some(Kind::Error),
            reply_serial: call.serial,
            destination: call.sender.clone(),
            error: error.into(),
            ..Self::default()
        }
    }

    /// Whether the sender of this call wants an answer.
    pub fn wants_answer(&self) -> bool {
        self.kind == Some(Kind::MethodCall) && self.flags & NO_REPLY == 0
    }

    /// The strings the body holds, in order, for the types lipc marshals:
    /// `s`, `o` and `g` yield one apiece and the fixed widths are stepped over.
    pub fn strings(&self) -> Vec<String> {
        let mut at = Decoder::new(&self.body);
        let mut out = Vec::new();
        for code in self.signature.bytes() {
            match code {
                b's' | b'o' => match at.string() {
                    Some(said) => out.push(said),
                    None => break,
                },
                b'g' => {
                    if at.signature().is_none() {
                        break;
                    }
                }
                b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd' => {
                    if !at.step(code) {
                        break;
                    }
                }
                // An array, a struct or a variant is not a shape lipc sends.
                _ => break,
            }
        }
        out
    }

    /// This message on the wire, little-endian, stamped `serial`.
    pub fn encode(&self, serial: u32) -> Vec<u8> {
        let mut out = Encoder::default();
        out.byte(LITTLE);
        out.byte(self.kind.map(Kind::code).unwrap_or(0));
        out.byte(self.flags);
        out.byte(VERSION);
        out.u32(self.body.len() as u32);
        out.u32(serial);

        out.align(4);
        let length_at = out.out.len();
        out.u32(0);
        out.align(8);
        let fields_at = out.out.len();
        for (code, said) in [
            (PATH, &self.path),
            (INTERFACE, &self.interface),
            (MEMBER, &self.member),
            (ERROR_NAME, &self.error),
            (DESTINATION, &self.destination),
            (SENDER, &self.sender),
        ] {
            if said.is_empty() {
                continue;
            }
            out.align(8);
            out.byte(code);
            out.signature(if code == PATH { "o" } else { "s" });
            out.string(said);
        }
        if self.reply_serial != 0 {
            out.align(8);
            out.byte(REPLY_SERIAL);
            out.signature("u");
            out.u32(self.reply_serial);
        }
        if !self.signature.is_empty() {
            out.align(8);
            out.byte(SIGNATURE);
            out.signature("g");
            out.signature(&self.signature);
        }
        let fields = (out.out.len() - fields_at) as u32;
        out.out[length_at..length_at + 4].copy_from_slice(&fields.to_le_bytes());

        out.align(8);
        out.out.extend_from_slice(&self.body);
        out.out
    }
}

/// The message at the head of `said`, and how many bytes it took. `None` on a
/// message that has not arrived whole.
pub fn decode(said: &[u8]) -> Option<(Message, usize)> {
    if said.len() < HEAD {
        return None;
    }
    if said[0] != LITTLE {
        return None;
    }
    let word = |at: usize| -> u32 {
        u32::from_le_bytes([said[at], said[at + 1], said[at + 2], said[at + 3]])
    };
    let body_len = word(4) as usize;
    let fields_len = word(12) as usize;
    let body_at = (HEAD + fields_len).div_ceil(8) * 8;
    let whole = body_at.checked_add(body_len)?;
    if said.len() < whole {
        return None;
    }

    let mut held = Message {
        kind: Kind::of_code(said[1]),
        flags: said[2],
        serial: word(8),
        body: said[body_at..whole].to_vec(),
        ..Message::default()
    };
    let mut at = Decoder::new(&said[HEAD..HEAD + fields_len]);
    while at.pos < at.said.len() {
        at.align(8);
        let Some(code) = at.byte() else { break };
        let Some(kind) = at.signature() else { break };
        match (code, kind.as_str()) {
            (PATH, _)
            | (INTERFACE, _)
            | (MEMBER, _)
            | (ERROR_NAME, _)
            | (DESTINATION, _)
            | (SENDER, _) => {
                let Some(said) = at.string() else { break };
                let field = match code {
                    PATH => &mut held.path,
                    INTERFACE => &mut held.interface,
                    MEMBER => &mut held.member,
                    ERROR_NAME => &mut held.error,
                    DESTINATION => &mut held.destination,
                    _ => &mut held.sender,
                };
                *field = said;
            }
            (REPLY_SERIAL, _) => match at.u32() {
                Some(serial) => held.reply_serial = serial,
                None => break,
            },
            (SIGNATURE, _) => match at.signature() {
                Some(said) => held.signature = said,
                None => break,
            },
            // A field this build does not read, stepped over by its own type.
            (_, kind) => {
                if !kind.bytes().all(|code| at.step(code)) {
                    break;
                }
            }
        }
    }
    Some((held, whole))
}

/// Bytes going out, little-endian.
#[derive(Default)]
struct Encoder {
    out: Vec<u8>,
}

impl Encoder {
    fn align(&mut self, to: usize) {
        while !self.out.len().is_multiple_of(to) {
            self.out.push(0);
        }
    }

    fn byte(&mut self, value: u8) {
        self.out.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.align(4);
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A `u32` length, the bytes, and a NUL.
    fn string(&mut self, said: &str) {
        self.u32(said.len() as u32);
        self.out.extend_from_slice(said.as_bytes());
        self.out.push(0);
    }

    /// A one-byte length, the bytes, and a NUL.
    fn signature(&mut self, said: &str) {
        self.byte(said.len() as u8);
        self.out.extend_from_slice(said.as_bytes());
        self.out.push(0);
    }
}

/// Bytes coming in, read at `pos`.
struct Decoder<'a> {
    said: &'a [u8],
    pos: usize,
}

impl<'a> Decoder<'a> {
    fn new(said: &'a [u8]) -> Self {
        Self { said, pos: 0 }
    }

    fn align(&mut self, to: usize) {
        self.pos = self.pos.div_ceil(to) * to;
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.said.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|said| said[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let said = self.take(4)?;
        Some(u32::from_le_bytes([said[0], said[1], said[2], said[3]]))
    }

    fn string(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        let said = self.take(len)?;
        self.byte()?;
        Some(String::from_utf8_lossy(said).into_owned())
    }

    fn signature(&mut self) -> Option<String> {
        let len = self.byte()? as usize;
        let said = self.take(len)?;
        self.byte()?;
        Some(String::from_utf8_lossy(said).into_owned())
    }

    /// Steps over one value of the type `code` names, answering whether it
    /// was there.
    fn step(&mut self, code: u8) -> bool {
        let width = match code {
            b'y' => 1,
            b'n' | b'q' => 2,
            b'b' | b'i' | b'u' => 4,
            b'x' | b't' | b'd' => 8,
            b's' | b'o' => return self.string().is_some(),
            b'g' => return self.signature().is_some(),
            _ => return false,
        };
        self.align(width);
        self.take(width).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The head of a call to lipc's own object.
    fn commit() -> Message {
        let mut out = Encoder::default();
        out.string("hello");
        out.string("/usr/bin/kb");
        Message {
            kind: Some(Kind::MethodCall),
            serial: 7,
            path: "/default".into(),
            interface: "com.readinglog.stats".into(),
            member: "setkeyboardCommitStr".into(),
            destination: "com.readinglog.stats".into(),
            sender: ":1.42".into(),
            signature: "ss".into(),
            body: out.out,
            ..Message::default()
        }
    }

    #[test]
    fn a_message_comes_back_off_its_own_wire() {
        let said = commit();
        let wire = said.encode(said.serial);
        let (back, took) = decode(&wire).expect("a whole message");
        assert_eq!(took, wire.len());
        assert_eq!(back, said);
    }

    /// Every field lands on the boundary its type takes, counted from the head
    /// of the message.
    #[test]
    fn every_field_lands_on_its_own_boundary() {
        let wire = commit().encode(7);
        assert_eq!(wire[0], LITTLE);
        assert_eq!(wire[3], VERSION);
        let fields = u32::from_le_bytes([wire[12], wire[13], wire[14], wire[15]]) as usize;
        let body_at = (HEAD + fields).div_ceil(8) * 8;
        assert_eq!(body_at % 8, 0);
        let body_len = u32::from_le_bytes([wire[4], wire[5], wire[6], wire[7]]) as usize;
        assert_eq!(wire.len(), body_at + body_len);
    }

    /// A message short of its last byte is not a message.
    #[test]
    fn a_part_message_is_no_message() {
        let wire = commit().encode(7);
        for cut in [0, 1, 15, HEAD, wire.len() - 1] {
            assert!(decode(&wire[..cut]).is_none(), "{cut}");
        }
        assert!(decode(&wire).is_some());
    }

    /// Two messages in one read are two messages.
    #[test]
    fn a_read_holding_two_messages_yields_both() {
        let mut wire = commit().encode(7);
        let second = wire.len();
        wire.extend_from_slice(&commit().encode(8));
        let (first, took) = decode(&wire).expect("the first");
        assert_eq!(took, second);
        assert_eq!(first.serial, 7);
        let (next, _) = decode(&wire[took..]).expect("the second");
        assert_eq!(next.serial, 8);
    }

    /// The body's strings come back in order, whatever else the signature
    /// carries.
    #[test]
    fn the_body_yields_its_strings_in_order() {
        assert_eq!(commit().strings(), ["hello", "/usr/bin/kb"]);

        let mut out = Encoder::default();
        out.u32(501);
        out.u32(501);
        out.string("委");
        let said = Message {
            signature: "uus".into(),
            body: out.out,
            ..Message::default()
        };
        assert_eq!(said.strings(), ["委"]);
    }

    /// An answer carries the call's serial and its sender, and a refusal names
    /// the error.
    #[test]
    fn an_answer_is_addressed_to_the_call() {
        let call = commit();
        let mut out = Encoder::default();
        out.u32(0);
        let answer = Message::answer(&call, "u", out.out);
        let (back, _) = decode(&answer.encode(1)).expect("a whole message");
        assert_eq!(back.kind, Some(Kind::MethodReturn));
        assert_eq!(back.reply_serial, call.serial);
        assert_eq!(back.destination, call.sender);
        assert_eq!(back.signature, "u");
        assert_eq!(back.body, 0u32.to_le_bytes());

        let refusal = Message::refuse(&call, "org.freedesktop.DBus.Error.UnknownMethod");
        let (back, _) = decode(&refusal.encode(2)).expect("a whole message");
        assert_eq!(back.kind, Some(Kind::Error));
        assert_eq!(back.error, "org.freedesktop.DBus.Error.UnknownMethod");
        assert_eq!(back.reply_serial, call.serial);
    }

    /// A call carrying the no-reply flag wants none.
    #[test]
    fn a_call_wanting_no_answer_says_so() {
        let mut call = commit();
        assert!(call.wants_answer());
        call.flags = NO_REPLY;
        assert!(!call.wants_answer());
        let mut signal = commit();
        signal.kind = Some(Kind::Signal);
        assert!(!signal.wants_answer());
    }

    /// A message in any other byte order is no message.
    #[test]
    fn a_message_in_another_order_is_no_message() {
        let mut wire = commit().encode(7);
        wire[0] = b'B';
        assert!(decode(&wire).is_none());
    }
}
