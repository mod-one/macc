//! One overall transport deadline, bounded responses and a numeric loopback endpoint.
use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
pub async fn send(root: &Path, request: &Request) -> Result<()> {
    request.validate()?;
    let addr = crate::coordinator::ipc::read_performer_ipc_addr(root)
        .ok_or_else(|| error("Live coordinator has no control endpoint; task was not changed"))?;
    let addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|_| error("Coordinator control endpoint must be a numeric loopback address"))?;
    if !addr.ip().is_loopback() {
        return Err(error("Coordinator control endpoint must be loopback"));
    }
    tokio::time::timeout(BUDGET,async {
        let mut stream=tokio::net::TcpStream::connect(addr).await.map_err(|e|error(format!("Connect coordinator control endpoint: {e}")))?;
        let mut bytes=serde_json::to_vec(request).map_err(|e|error(e.to_string()))?;
        bytes.push(b'\n');
        stream.write_all(&bytes).await.map_err(|e|error(format!("Write control request: {e}")))?;
        let mut reader=BufReader::new(stream.take(65537));
        let mut line=String::new();
        reader.read_line(&mut line).await.map_err(|e|error(format!("Read control acknowledgement: {e}")))?;
        if line.len()>65536 || !line.ends_with('\n') {return Err(error("Invalid or oversized coordinator control acknowledgement"));}
        let reply:Reply=serde_json::from_str(&line).map_err(|e|error(format!("Invalid control acknowledgement: {e}")))?;
        if reply.request_id!=request.request_id {return Err(error("Coordinator acknowledged a different control request"));}
        if let Some(reason)=reply.error {return Err(error(reason));}
        Ok(())
    }).await.map_err(|_|error(format!("Unblock acknowledgement timed out (request {}); outcome may be pending: reload task state before retrying",request.request_id)))?
}
/// Synchronous clients run transport on a dedicated thread. Async callers sharing
/// the listener's executor must use `send().await` or `spawn_blocking` instead.
pub fn send_sync(root: &Path, request: &Request) -> Result<()> {
    let root = root.to_path_buf();
    let request = request.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| error(e.to_string()))?;
        runtime.block_on(send(&root, &request))
    })
    .join()
    .map_err(|_| error("Coordinator control transport thread failed"))?
}
