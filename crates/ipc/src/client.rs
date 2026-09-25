//! A blocking client: one connection, requests answered in order, events
//! delivered between answers. Used by the CLI and by tests.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::time::{Duration, Instant};

use crate::{Command, Event, Incoming, MAX_MESSAGE, PROTOCOL_VERSION, Request, Response};

#[derive(Debug)]
pub enum ConnectError {
    /// No host listens for this data folder.
    NotRunning,
    Io(io::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::NotRunning => write!(f, "no host is running"),
            ConnectError::Io(e) => write!(f, "{e}"),
        }
    }
}

use crate::transport::{self, Stream};

pub struct Client {
    reader: BufReader<Stream>,
    writer: Stream,
    counter: u64,
    events: VecDeque<Event>,
    prefix: String,
}

impl Client {
    pub fn connect(endpoint: &str) -> Result<Client, ConnectError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match transport::open(endpoint) {
                Ok(stream) => {
                    let writer = stream.try_clone().map_err(ConnectError::Io)?;
                    let prefix = format!("c{}-{}", std::process::id(), nanos());
                    return Ok(Client {
                        reader: BufReader::new(stream),
                        writer,
                        counter: 0,
                        events: VecDeque::new(),
                        prefix,
                    });
                }
                Err(e) if is_not_running(&e) => return Err(ConnectError::NotRunning),
                // Every pipe instance is busy for a moment: try again shortly.
                Err(e) if transport::is_busy(&e) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(e) => return Err(ConnectError::Io(e)),
            }
        }
    }

    /// A fresh request id.
    pub fn next_id(&mut self) -> String {
        self.counter += 1;
        format!("{}-{}", self.prefix, self.counter)
    }

    /// Sends a command and waits for its answer. Events that arrive first
    /// are kept for [`Client::next_event`].
    pub fn request(&mut self, id: Option<String>, command: Command) -> io::Result<Response> {
        let id = id.unwrap_or_else(|| self.next_id());
        let request = Request { v: PROTOCOL_VERSION, id: id.clone(), command };
        let line = serde_json::to_string(&request).map_err(io::Error::other)?;
        self.send_line(&line)?;
        self.wait_for(&id)
    }

    /// Sends a raw line (for protocol tests) and waits for the answer to `id`.
    pub fn request_raw(&mut self, line: &str, id: &str) -> io::Result<Response> {
        self.send_line(line)?;
        self.wait_for(id)
    }

    fn send_line(&mut self, line: &str) -> io::Result<()> {
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    fn wait_for(&mut self, id: &str) -> io::Result<Response> {
        loop {
            match self.read()? {
                Some(Incoming::Response(response)) if response.re == id => return Ok(response),
                Some(Incoming::Response(_)) => {}
                Some(Incoming::Event(event)) => self.events.push_back(event),
                None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the host closed the connection")),
            }
        }
    }

    /// The next pushed event; None when the host closed the connection.
    pub fn next_event(&mut self) -> io::Result<Option<Event>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        loop {
            match self.read()? {
                Some(Incoming::Event(event)) => return Ok(Some(event)),
                Some(Incoming::Response(_)) => {}
                None => return Ok(None),
            }
        }
    }

    fn read(&mut self) -> io::Result<Option<Incoming>> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = io::Read::take(&mut self.reader, MAX_MESSAGE as u64 + 2).read_line(&mut line)?;
            if n == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            return Incoming::parse(line.trim_end())
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn is_not_running(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound || e.kind() == io::ErrorKind::ConnectionRefused
}
