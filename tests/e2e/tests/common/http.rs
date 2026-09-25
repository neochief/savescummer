//! A tiny HTTP server on localhost standing in for GitHub and Steam's CDN:
//! routes with bodies and ETags that tests change while the host runs, and
//! a log of what the host asked for.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Route {
    pub status: u16,
    pub body: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub path: String,
    pub if_none_match: Option<String>,
}

#[derive(Default)]
struct Shared {
    routes: HashMap<String, Route>,
    requests: Vec<Request>,
}

pub struct Server {
    pub base: String,
    shared: Arc<Mutex<Shared>>,
}

impl Server {
    pub fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let state = state.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    if reader.read_line(&mut first).is_err() {
                        return;
                    }
                    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let mut if_none_match = None;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':')
                            && name.eq_ignore_ascii_case("if-none-match")
                        {
                            if_none_match = Some(value.trim().to_string());
                        }
                    }
                    let route = {
                        let mut shared = state.lock().unwrap();
                        shared.requests.push(Request { path: path.clone(), if_none_match: if_none_match.clone() });
                        shared.routes.get(&path).cloned()
                    };
                    let route = route.unwrap_or(Route { status: 404, body: b"not found".to_vec(), etag: None });
                    let not_modified = route.status == 200 && route.etag.is_some() && route.etag == if_none_match;
                    let (status, body) = if not_modified { (304, Vec::new()) } else { (route.status, route.body) };
                    let mut head =
                        format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                    if let Some(etag) = &route.etag {
                        head.push_str(&format!("ETag: {etag}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body);
                });
            }
        });
        Server { base, shared }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn serve(&self, path: &str, status: u16, body: impl Into<Vec<u8>>, etag: Option<&str>) {
        let route = Route { status, body: body.into(), etag: etag.map(str::to_string) };
        self.shared.lock().unwrap().routes.insert(path.to_string(), route);
    }

    pub fn requests(&self, path: &str) -> Vec<Request> {
        self.shared.lock().unwrap().requests.iter().filter(|r| r.path == path).cloned().collect()
    }

    pub fn all_requests(&self) -> Vec<Request> {
        self.shared.lock().unwrap().requests.clone()
    }
}
