//! Serves the protocol: one task per connection, each request answered as
//! it completes (clients match answers by request id), and state pushed to
//! watchers. Requests never wait for a scan unless they asked for one.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use savescummer_core::{ErrorKind, Failure};
use savescummer_ipc::{
    Command, Event, EventBody, Hello, HotkeyTargetInfo, MAX_MESSAGE, OpStatus, PROTOCOL_VERSION, Request, Response,
};

use crate::host::Host;
use crate::{library, ops, queries};

/// Accepts connections until the process exits.
#[cfg(windows)]
pub async fn serve(host: Arc<Host>, first: tokio::net::windows::named_pipe::NamedPipeServer) {
    use tokio::net::windows::named_pipe::ServerOptions;
    let mut server = first;
    loop {
        if server.connect().await.is_err() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let connected = server;
        server = match ServerOptions::new().reject_remote_clients(true).create(&host.endpoint) {
            Ok(s) => s,
            Err(e) => {
                crate::trace(&format!("can't create another pipe instance: {e}"));
                return;
            }
        };
        tokio::spawn(connection(host.clone(), connected));
    }
}

/// Creates the first pipe instance; fails when another host owns the name.
#[cfg(windows)]
pub fn bind(endpoint: &str) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(endpoint)
}

#[cfg(unix)]
pub fn bind(endpoint: &str) -> std::io::Result<tokio::net::UnixListener> {
    let _ = std::fs::remove_file(endpoint);
    tokio::net::UnixListener::bind(endpoint)
}

#[cfg(unix)]
pub async fn serve(host: Arc<Host>, listener: tokio::net::UnixListener) {
    loop {
        if let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(connection(host.clone(), stream));
        }
    }
}

async fn connection<S: AsyncRead + AsyncWrite + Send + 'static>(host: Arc<Host>, stream: S) {
    let (reader, mut writer) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer_task = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() || writer.write_all(b"\n").await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
    });
    let mut lines = BufReader::new(reader).lines();
    let mut watching = false;
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            _ => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_MESSAGE {
            send(&tx, &error_response("?", Failure::new(ErrorKind::TooLarge, "the request is too large")));
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                send(&tx, &error_response("?", Failure::new(ErrorKind::InvalidRequest, e.to_string())));
                continue;
            }
        };
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let version = value.get("v").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if version != PROTOCOL_VERSION {
            // Refused clearly, never half-understood.
            send(
                &tx,
                &error_response(
                    &id,
                    Failure::new(
                        ErrorKind::VersionMismatch,
                        format!("this host speaks protocol {PROTOCOL_VERSION}, the client {version}"),
                    ),
                ),
            );
            break;
        }
        let request: Request = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => {
                send(&tx, &error_response(&id, Failure::new(ErrorKind::InvalidRequest, e.to_string())));
                continue;
            }
        };
        if matches!(request.command, Command::Watch) {
            send(&tx, &ok_response(&id, serde_json::json!({})));
            if !watching {
                watching = true;
                // A UI attaching: art it may be missing is fetched now.
                crate::artwork::request(&host);
                tokio::spawn(watch(host.clone(), tx.clone()));
            }
            continue;
        }
        let host = host.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let id = request.id.clone();
            let response = tokio::task::spawn_blocking(move || dispatch(&host, request)).await.unwrap_or_else(|e| {
                error_response(&id, Failure::new(ErrorKind::Io, format!("the request failed: {e}")))
            });
            send(&tx, &response);
        });
    }
    drop(tx);
    let _ = writer_task.await;
}

/// Pushes the current state, then every new state (coalesced), label
/// changes and the shutdown notice.
async fn watch(host: Arc<Host>, tx: mpsc::UnboundedSender<String>) {
    let mut states = host.state_tx.subscribe();
    let mut events = host.events_tx.subscribe();
    let state = states.borrow_and_update().clone();
    if !send(&tx, &Event { v: PROTOCOL_VERSION, body: EventBody::State { state: Box::new((*state).clone()) } }) {
        return;
    }
    loop {
        tokio::select! {
            changed = states.changed() => {
                if changed.is_err() { return; }
                let state = states.borrow_and_update().clone();
                if !send(&tx, &Event { v: PROTOCOL_VERSION, body: EventBody::State { state: Box::new((*state).clone()) } }) {
                    return;
                }
            }
            event = events.recv() => {
                match event {
                    Ok(body) => {
                        let last = matches!(body, EventBody::Shutdown);
                        if !send(&tx, &Event { v: PROTOCOL_VERSION, body }) || last {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return,
                }
            }
        }
    }
}

fn send<T: Serialize>(tx: &mpsc::UnboundedSender<String>, message: &T) -> bool {
    let line = serde_json::to_string(message).expect("messages serialize");
    let line = if line.len() > MAX_MESSAGE {
        serde_json::to_string(&error_response("?", Failure::new(ErrorKind::TooLarge, "the reply would be too large")))
            .unwrap()
    } else {
        line
    };
    tx.send(line).is_ok()
}

fn ok_response(id: &str, result: serde_json::Value) -> Response {
    Response { v: PROTOCOL_VERSION, re: id.to_string(), ok: true, result: Some(result), error: None }
}

fn error_response(id: &str, error: Failure) -> Response {
    Response { v: PROTOCOL_VERSION, re: id.to_string(), ok: false, result: None, error: Some(error) }
}

fn json<T: Serialize>(value: T) -> Result<serde_json::Value, Failure> {
    Ok(serde_json::to_value(value).expect("results serialize"))
}

/// Answers one request. Runs on a blocking thread.
pub fn dispatch(host: &Arc<Host>, request: Request) -> Response {
    let id = request.id.clone();
    let result = answer(host, &id, request.command);
    match result {
        Ok(value) => ok_response(&id, value),
        Err(failure) => error_response(&id, failure),
    }
}

fn answer(host: &Arc<Host>, request_id: &str, command: Command) -> Result<serde_json::Value, Failure> {
    match command {
        Command::Hello { .. } => json(Hello {
            protocol: PROTOCOL_VERSION,
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            instance: host.instance.clone(),
        }),
        Command::State => json(&*host.current_state()),
        Command::Watch => unreachable!("handled by the connection"),
        Command::History { game, cursor, limit } => json(queries::history(host, &game, cursor.as_deref(), limit)?),
        Command::FlushPreview { game, cursor, limit } => {
            json(queries::flush_preview(host, &game, cursor.as_deref(), limit)?)
        }
        Command::Outcome { operation, wait } => {
            let deadline = std::time::Instant::now() + Duration::from_secs(600);
            let mut states = host.state_tx.subscribe();
            loop {
                let op = ops::find(host, &operation)
                    .ok_or_else(|| Failure::new(ErrorKind::NotFound, "no such operation"))?;
                if !wait || op.status.is_final() || std::time::Instant::now() > deadline {
                    return json(op);
                }
                // Wake on the next state change, or poll.
                let _ = futures_wait(&mut states, Duration::from_millis(100));
            }
        }
        Command::SaveSet { game } => json(queries::save_set(host, &game)?),
        Command::Catalog => json(queries::catalog(host)),
        Command::HostRuns { limit } => json(queries::host_runs(host, limit)?),
        Command::HotkeyTarget => {
            let inner = host.lock();
            let target = crate::host::hotkey_target(&inner);
            json(HotkeyTargetInfo {
                game: target.as_ref().map(|t| t.0.clone()),
                source: target.map(|t| t.1.to_string()),
            })
        }
        Command::Save { game, label } => {
            json(ops::submit(host, request_id, &game, ops::Request::Save { label }, false)?)
        }
        Command::Load { game, checkpoint } => {
            json(ops::submit(host, request_id, &game, ops::Request::Load { checkpoint }, false)?)
        }
        Command::Revert { game, checkpoint } => {
            json(ops::submit(host, request_id, &game, ops::Request::Revert { checkpoint }, false)?)
        }
        Command::Delete { game, checkpoint } => {
            json(ops::submit(host, request_id, &game, ops::Request::Delete { checkpoint }, false)?)
        }
        Command::CancelDelete { operation } => json(ops::cancel_delete(host, &operation)?),
        Command::Flush { game } => json(ops::submit(host, request_id, &game, ops::Request::Flush, false)?),
        Command::MoveStore { path } => json(ops::move_store(host, request_id, &path)?),
        Command::Retry { game } => {
            let game = host.find_game(&host.lock(), &game)?;
            if !host.lock().blocked.contains_key(&game) {
                return Err(Failure::new(
                    ErrorKind::InvalidRequest,
                    "the game isn't blocked; send the command again instead",
                )
                .game(&game));
            }
            crate::recovery::retry(host, &game)?;
            let mut inner = host.lock();
            host.refresh_cache(&mut inner, &game);
            host.publish(&mut inner);
            json(serde_json::json!({ "game": game }))
        }
        Command::SetLabel { checkpoint, label } => queries::set_label(host, &checkpoint, label.as_deref()),
        Command::AddGame { name, executable, save_location } => {
            let id = library::add_custom(host, &name, &executable, &save_location)?;
            json(serde_json::json!({ "game": id }))
        }
        Command::Configure { game, name, executable, save_location, reset_executable, reset_save_location } => {
            let game = host.find_game(&host.lock(), &game)?;
            library::configure(
                host,
                &game,
                library::ConfigureRequest {
                    name: name.as_deref(),
                    executable: executable.as_deref(),
                    save_location: save_location.as_deref(),
                    reset_executable,
                    reset_save_location,
                },
            )?;
            json(serde_json::json!({ "game": game }))
        }
        Command::Scan { full } => {
            let job = host.scans.request(full, true, "requested");
            let result = host
                .scans
                .wait(job, Duration::from_secs(600))
                .ok_or_else(|| Failure::new(ErrorKind::ShuttingDown, "the scan didn't finish"))?;
            json(result)
        }
        Command::Settings { play_sounds, launch_on_startup } => queries::settings(host, play_sounds, launch_on_startup),
        Command::UiReport { focused, visible, selected } => {
            let scan = {
                let mut inner = host.lock();
                let gained = focused && !inner.ui.focused;
                inner.ui = crate::host::UiReport { focused, visible, selected };
                // Showing or focusing the window runs an install scan, at
                // most once per cooldown.
                let cooldown = Duration::from_secs(host.opts.focus_scan_cooldown_secs);
                let due = inner.last_focus_scan.is_none_or(|t| t.elapsed() >= cooldown);
                let scan = gained && due && inner.phase == savescummer_ipc::Phase::Ready;
                if scan {
                    inner.last_focus_scan = Some(std::time::Instant::now());
                }
                host.publish(&mut inner);
                scan
            };
            if scan {
                host.scans.request(false, false, "the window gained focus");
            }
            json(serde_json::json!({ "scan": scan }))
        }
        Command::Open { target, resolve_only } => json(queries::open(host, &target, resolve_only)?),
        Command::CatalogRefresh => queries::catalog_refresh(host),
        Command::Hotkey { action } => {
            let op = crate::feedback::hotkey(host, request_id, action)?;
            json(op)
        }
        Command::Shutdown => {
            host.request_shutdown();
            json(serde_json::json!({ "shutting_down": true }))
        }
    }
}

/// Blocks until the state changes or the timeout passes.
fn futures_wait(states: &mut tokio::sync::watch::Receiver<Arc<savescummer_ipc::State>>, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if states.has_changed().unwrap_or(true) {
            states.borrow_and_update();
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[allow(dead_code)]
fn is_final(status: OpStatus) -> bool {
    status.is_final()
}
