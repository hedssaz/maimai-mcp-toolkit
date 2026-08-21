use std::io;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};

pub(super) struct MockResponse {
    pub(super) status: u16,
    pub(super) body: String,
}

pub(super) async fn server(
    response: MockResponse,
) -> Result<(String, oneshot::Receiver<String>), io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let _result = async {
            let (mut stream, _) = listener.accept().await?;
            let request = read_request(&mut stream).await?;
            let _ = sender.send(request);
            let reason = if response.status >= 400 { "Error" } else { "OK" };
            let encoded = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.status,
                response.body.len(),
                response.body,
            );
            stream.write_all(encoded.as_bytes()).await?;
            stream.shutdown().await
        }
        .await;
    });
    Ok((format!("http://{address}/api/"), receiver))
}

pub(super) fn cover_base(api_base: &str) -> String {
    api_base.replace("/api/", "/covers/")
}

pub(super) async fn keep_alive_server() -> Result<(String, mpsc::Receiver<String>), io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(2);
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        for index in 0..2 {
            let Ok(request) = read_request(&mut stream).await else {
                return;
            };
            if sender.send(request).await.is_err() {
                return;
            }
            let connection = if index == 0 { "keep-alive" } else { "close" };
            let body = "[]";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n{body}",
                body.len()
            );
            if stream.write_all(response.as_bytes()).await.is_err() {
                return;
            }
        }
    });
    Ok((format!("http://{address}/api/"), receiver))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "mock request ended before headers",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}
