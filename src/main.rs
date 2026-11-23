mod hardware;
mod optimize;
mod taskmanager;

use hardware::{scan_hardware, scan_hardware_isolated, HardwareReport};
use serde::Deserialize;
use std::io::Read;
use std::sync::{Arc, Mutex, RwLock};
use taskmanager::TaskManagerState;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX_HTML: &str = include_str!("../index.html");

#[derive(Deserialize)]
struct StartupToggleReq {
    name: String,
    enabled: bool,
}

#[derive(Deserialize)]
struct TweakToggleReq {
    id: String,
    optimized: bool,
}

#[derive(Deserialize)]
struct KillGroupReq {
    name: String,
}

fn bind_server() -> Server {
    for port in 8080..=8100 {
        let addr = format!("127.0.0.1:{port}");
        if let Ok(server) = Server::http(&addr) {
            return server;
        }
    }
    Server::http("127.0.0.1:0").expect("Failed to bind local HTTP server on any port")
}

fn open_browser(url: &str) {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

fn json_response<T: serde::Serialize>(data: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|e| {
        format!(r#"{{"error":"JSON serialization failed: {e}"}}"#).into_bytes()
    });
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
        .expect("valid header");
    let cc = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).expect("valid header");
    Response::from_data(body)
        .with_status_code(StatusCode(200))
        .with_header(ct)
        .with_header(cc)
}

fn json_error(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&serde_json::json!({ "error": msg }))
        .unwrap_or_else(|_| br#"{"error":"unknown"}"#.to_vec());
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
        .expect("valid header");
    let cc = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).expect("valid header");
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(ct)
        .with_header(cc)
}

fn check_session_token(request: &Request, expected: &str) -> bool {
    request.headers().iter().any(|h| {
        h.field.equiv("X-Session-Token") && h.value.as_str().trim() == expected
    })
}

fn read_json_body<T: for<'de> Deserialize<'de>>(request: &mut Request) -> Result<T, String> {
    let mut buf = Vec::new();
    request
        .as_reader()
        .take(4096)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Gagal membaca body: {e}"))?;
    serde_json::from_slice::<T>(&buf).map_err(|e| format!("JSON tidak valid: {e}"))
}

fn html_response(html: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
        .expect("valid header");
    Response::from_string(html)
        .with_status_code(StatusCode(200))
        .with_header(ct)
}

fn main() {
    if std::env::args().any(|a| a == "--wmi-scan") {
        let report = scan_hardware();
        let _ = serde_json::to_writer(std::io::stdout(), &report);
        return;
    }

    let no_open = std::env::args().any(|a| a == "--no-open");

    println!("Scanning hardware & Device Manager via WMI...");
    let initial_hw = scan_hardware_isolated();
    println!(
        "Scan complete in {} ms ({} PnP devices, {} errors).",
        initial_hw.scan_duration_ms,
        initial_hw.device_manager.total_devices,
        initial_hw.device_manager.error_count
    );

    let hw_cache: Arc<RwLock<HardwareReport>> = Arc::new(RwLock::new(initial_hw));
    let tm_state: Arc<Mutex<TaskManagerState>> = Arc::new(Mutex::new(TaskManagerState::new()));
    let session_token = Arc::new(optimize::generate_session_token());
    let rendered_html = Arc::new(INDEX_HTML.replace("__HW_MONITOR_TOKEN__", &session_token));

    let server = bind_server();
    let bound_port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(8080);
    let url = format!("http://127.0.0.1:{bound_port}");
    println!("Server running at {url}");

    if !no_open {
        open_browser(&url);
    }

    for mut request in server.incoming_requests() {
        let hw_cache = Arc::clone(&hw_cache);
        let tm_state = Arc::clone(&tm_state);
        let session_token = Arc::clone(&session_token);
        let rendered_html = Arc::clone(&rendered_html);

        std::thread::spawn(move || {
            let method = request.method().clone();
            let raw_url = request.url().to_string();
            let (path, query) = match raw_url.split_once('?') {
                Some((p, q)) => (p, q),
                None => (raw_url.as_str(), ""),
            };

            match (method, path) {
                (Method::Get, "/") | (Method::Get, "/index.html") => {
                    let _ = request.respond(html_response(&rendered_html));
                }
                (Method::Get, "/api/hardware") | (Method::Get, "/api/rescan") => {
                    let should_rescan = path == "/api/rescan"
                        || query.split('&').any(|kv| kv == "rescan=1" || kv == "rescan=true");
                    if should_rescan {
                        let fresh = scan_hardware_isolated();
                        if let Ok(mut guard) = hw_cache.write() {
                            *guard = fresh.clone();
                        }
                        let _ = request.respond(json_response(&fresh));
                    } else if let Ok(guard) = hw_cache.read() {
                        let _ = request.respond(json_response(&*guard));
                    } else {
                        let _ = request.respond(json_error(500, "Cache lock poisoned"));
                    }
                }
                (Method::Get, "/api/taskmanager") => {
                    if let Ok(mut guard) = tm_state.lock() {
                        let snap = guard.snapshot();
                        let _ = request.respond(json_response(&snap));
                    } else {
                        let _ = request.respond(json_error(500, "TaskManager lock poisoned"));
                    }
                }
                (Method::Get, "/api/optimize/status") => {
                    let status = optimize::get_status();
                    let _ = request.respond(json_response(&status));
                }
                (Method::Post, p) if p.starts_with("/api/optimize/") => {
                    if !check_session_token(&request, &session_token) {
                        let _ = request.respond(json_error(403, "Invalid or missing session token"));
                        return;
                    }
                    match p {
                        "/api/optimize/startup" => match read_json_body::<StartupToggleReq>(&mut request) {
                            Ok(body) => match optimize::set_startup_enabled(&body.name, body.enabled) {
                                Ok(()) => {
                                    let _ = request.respond(json_response(&serde_json::json!({ "ok": true })));
                                }
                                Err(e) => {
                                    let _ = request.respond(json_error(400, &e));
                                }
                            },
                            Err(e) => {
                                let _ = request.respond(json_error(400, &e));
                            }
                        },
                        "/api/optimize/tweak" => match read_json_body::<TweakToggleReq>(&mut request) {
                            Ok(body) => match optimize::apply_tweak(&body.id, body.optimized) {
                                Ok(tweak) => {
                                    let _ = request.respond(json_response(&tweak));
                                }
                                Err(e) => {
                                    let _ = request.respond(json_error(400, &e));
                                }
                            },
                            Err(e) => {
                                let _ = request.respond(json_error(400, &e));
                            }
                        },
                        "/api/optimize/clean-temp" => {
                            let res = optimize::clean_temp_dir();
                            let _ = request.respond(json_response(&res));
                        }
                        "/api/optimize/kill-group" => match read_json_body::<KillGroupReq>(&mut request) {
                            Ok(body) => match optimize::kill_process_group(&body.name) {
                                Ok(res) => {
                                    let _ = request.respond(json_response(&res));
                                }
                                Err(e) => {
                                    let _ = request.respond(json_error(400, &e));
                                }
                            },
                            Err(e) => {
                                let _ = request.respond(json_error(400, &e));
                            }
                        },
                        _ => {
                            let _ = request.respond(json_error(404, "Not Found"));
                        }
                    }
                }
                (Method::Get, _) => {
                    let _ = request.respond(json_error(404, "Not Found"));
                }
                _ => {
                    let _ = request.respond(json_error(405, "Method Not Allowed"));
                }
            }
        });
    }
}
