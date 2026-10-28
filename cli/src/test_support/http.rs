//! Local HTTP fixtures own their thread and have bounded shutdown and I/O.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub(crate) struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    pub fn start(body: Vec<u8>, content_type: &'static str) -> Option<Self> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("Bind fixture HTTP server: {error}"),
        };
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                let deadline = Instant::now() + Duration::from_millis(500);
                let mut request = Vec::new();
                let mut complete = false;
                while !worker_stop.load(Ordering::Relaxed)
                    && Instant::now() < deadline
                    && request.len() < 16384
                {
                    let mut bytes = [0; 512];
                    match stream.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            request.extend_from_slice(&bytes[..n]);
                            if request.windows(4).any(|w| w == b"\r\n\r\n")
                                || request.windows(2).any(|w| w == b"\n\n")
                            {
                                complete = true;
                                break;
                            }
                        }
                    }
                }
                if !complete {
                    continue;
                }
                let header=format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                if stream.write_all(header.as_bytes()).is_ok() {
                    let _ = stream.write_all(&body);
                }
            }
        });
        Some(Self {
            port,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("fixture HTTP worker panicked");
        }
    }
}
#[test]
fn idle_clients_cannot_hold_fixture_shutdown_open() {
    let Some(server) = Server::start(b"ok".to_vec(), "text/plain") else {
        return;
    };
    let _idle = std::net::TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    let before = Instant::now();
    drop(server);
    assert!(before.elapsed() < Duration::from_secs(2));
}
