//! A lipc service holding the properties the on-screen keyboard sets. lipc is
//! the system D-Bus: a property set is a `set<Property>Str` call on
//! `/default`, answered by a `u` status of zero.

mod wire;

use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use wire::{Kind, Message};

/// The system bus.
const SOCKET: &str = "/var/run/dbus/system_bus_socket";

/// The bus's own name, object and interface.
const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The object every lipc service answers on.
const OBJECT: &str = "/default";

/// What a property's method name opens and closes with.
const SET: &str = "set";
const GET: &str = "get";
const STR: &str = "Str";

/// `RequestName`'s answer for a name held outright.
const PRIMARY_OWNER: u32 = 1;

/// `RequestName`'s flags: no queue behind a name held elsewhere.
const DO_NOT_QUEUE: u32 = 0x4;

/// How long [`Service::open`]'s three round trips wait.
const HANDSHAKE: Duration = Duration::from_secs(3);

/// How much of a read is taken at a time.
const CHUNK: usize = 4096;

/// One property set on this service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    pub property: String,
    pub value: String,
}

pub struct Service {
    sock: UnixStream,
    /// Bytes read and not yet whole messages.
    inbox: Vec<u8>,
    /// Messages read while [`Service::round_trip`] waited for another.
    held: Vec<Message>,
    serial: u32,
    /// The name [`Service::open`] took on the bus.
    name: String,
}

impl Service {
    /// Connects to the system bus and takes `name`.
    pub fn open(name: &str) -> Result<Self> {
        let sock = UnixStream::connect(SOCKET).with_context(|| format!("connect {SOCKET}"))?;
        sock.set_read_timeout(Some(HANDSHAKE))?;
        let mut service = Self {
            sock,
            inbox: Vec::new(),
            held: Vec::new(),
            serial: 0,
            name: name.into(),
        };
        service.authenticate()?;
        let hello = Message::call(BUS, BUS_PATH, BUS, "Hello");
        service.round_trip(hello).context("Hello")?;
        let mut body = Vec::new();
        push_string(&mut body, name);
        push_u32(&mut body, DO_NOT_QUEUE);
        let mut request = Message::call(BUS, BUS_PATH, BUS, "RequestName");
        request.signature = "su".into();
        request.body = body;
        let answer = service.round_trip(request).context("RequestName")?;
        match first_u32(&answer.body) {
            Some(PRIMARY_OWNER) => {}
            said => bail!("RequestName {name} answered {said:?}"),
        }
        service.sock.set_nonblocking(true)?;
        Ok(service)
    }

    /// The socket, for a `poll(2)` beside the input devices.
    pub fn raw_fd(&self) -> RawFd {
        self.sock.as_raw_fd()
    }

    /// The name this service holds.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every property set that has arrived, each answered as it is read.
    pub fn drain(&mut self) -> Vec<Set> {
        self.fill();
        let mut out = Vec::new();
        for said in std::mem::take(&mut self.held) {
            out.extend(self.answer(&said));
        }
        while let Some((said, took)) = wire::decode(&self.inbox) {
            self.inbox.drain(..took);
            out.extend(self.answer(&said));
        }
        out
    }

    /// Reads whatever is waiting, without blocking.
    fn fill(&mut self) {
        let mut chunk = [0u8; CHUNK];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => return,
                Ok(read) => self.inbox.extend_from_slice(&chunk[..read]),
                Err(err) if err.kind() == ErrorKind::WouldBlock => return,
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => {
                    eprintln!("?? lipc: read: {err}");
                    return;
                }
            }
        }
    }

    /// Answers one call, and names the property a set carried.
    fn answer(&mut self, said: &Message) -> Option<Set> {
        if said.kind != Some(Kind::MethodCall) {
            return None;
        }
        let set = (said.path == OBJECT)
            .then(|| property(&said.member, SET))
            .flatten();
        let get = (said.path == OBJECT)
            .then(|| property(&said.member, GET))
            .flatten();
        if said.wants_answer() {
            let answer = match (set.is_some(), get.is_some()) {
                // A status of zero, and nothing else, answers a set.
                (true, _) => Message::answer(said, "u", u32_body(0)),
                // A get takes the status and the value after it.
                (_, true) => {
                    let mut body = u32_body(0);
                    push_string(&mut body, "");
                    Message::answer(said, "us", body)
                }
                _ => Message::refuse(said, "org.freedesktop.DBus.Error.UnknownMethod"),
            };
            self.send(answer);
        }
        set.map(|property| Set {
            property,
            value: said.strings().first().cloned().unwrap_or_default(),
        })
    }

    /// Writes `said` under the next serial.
    fn send(&mut self, said: Message) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        let serial = self.serial;
        if let Err(err) = self.sock.write_all(&said.encode(serial)) {
            eprintln!("?? lipc: write {}: {err}", said.member);
        }
        serial
    }

    /// `\0`, `AUTH EXTERNAL <uid>` and `BEGIN`.
    fn authenticate(&mut self) -> Result<()> {
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|byte| format!("{byte:02X}")).collect();
        self.sock.write_all(b"\0")?;
        self.sock
            .write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let said = self.line()?;
        if !said.starts_with("OK") {
            bail!("the bus refused EXTERNAL: {said}");
        }
        self.sock.write_all(b"BEGIN\r\n")?;
        Ok(())
    }

    /// One `\r\n` line of the handshake.
    fn line(&mut self) -> Result<String> {
        let mut said = Vec::new();
        let mut byte = [0u8; 1];
        while !said.ends_with(b"\r\n") {
            if self.sock.read(&mut byte)? == 0 {
                bail!("the bus closed the connection");
            }
            said.push(byte[0]);
            if said.len() > CHUNK {
                bail!("the bus answered no line");
            }
        }
        Ok(String::from_utf8_lossy(&said).trim().into())
    }

    /// Sends `said` and waits for the answer to it, holding back every other
    /// message for [`Service::drain`].
    fn round_trip(&mut self, said: Message) -> Result<Message> {
        let member = said.member.clone();
        let serial = self.send(said);
        let until = Instant::now() + HANDSHAKE;
        while Instant::now() < until {
            let mut chunk = [0u8; CHUNK];
            match self.sock.read(&mut chunk) {
                Ok(0) => bail!("the bus closed the connection"),
                Ok(read) => self.inbox.extend_from_slice(&chunk[..read]),
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => return Err(err).context("read the bus"),
            }
            while let Some((said, took)) = wire::decode(&self.inbox) {
                self.inbox.drain(..took);
                if said.reply_serial == serial {
                    return match said.kind {
                        Some(Kind::Error) => bail!("{member}: {}", said.error),
                        _ => Ok(said),
                    };
                }
                self.held.push(said);
            }
        }
        bail!("{member}: the bus did not answer")
    }
}

/// The property `member` names, where it opens with `opens` and closes with
/// [`STR`].
fn property(member: &str, opens: &str) -> Option<String> {
    let held = member.strip_prefix(opens)?.strip_suffix(STR)?;
    (!held.is_empty()).then(|| held.to_string())
}

/// A body holding one `u`.
fn u32_body(value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32(&mut out, value);
    out
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_string(out: &mut Vec<u8>, said: &str) {
    push_u32(out, said.len() as u32);
    out.extend_from_slice(said.as_bytes());
    out.push(0);
}

/// The `u` a body opens with.
fn first_u32(body: &[u8]) -> Option<u32> {
    let bytes = body.get(..4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_name_carries_the_property_between_its_ends() {
        assert_eq!(
            property("setkeyboardCommitStr", SET).as_deref(),
            Some("keyboardCommit")
        );
        assert_eq!(
            property("getkeyboardGetSurroundStr", GET).as_deref(),
            Some("keyboardGetSurround")
        );
        assert_eq!(property("setkeyboardCommitStr", GET), None);
        assert_eq!(property("setkeyboardCommitInt", SET), None);
        assert_eq!(property("setStr", SET), None);
    }

    #[test]
    fn a_body_of_one_word_reads_back_as_that_word() {
        assert_eq!(first_u32(&u32_body(0)), Some(0));
        assert_eq!(first_u32(&u32_body(PRIMARY_OWNER)), Some(PRIMARY_OWNER));
        assert_eq!(first_u32(&[]), None);
    }

    /// A string is a `u32` length, the bytes, and a NUL.
    #[test]
    fn a_string_carries_its_length_and_a_nul() {
        let mut out = Vec::new();
        push_string(&mut out, "abc");
        assert_eq!(out, [3, 0, 0, 0, b'a', b'b', b'c', 0]);
    }
}
