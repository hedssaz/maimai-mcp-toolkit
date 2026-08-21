use std::{collections::BTreeMap, io, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinHandle,
};
use url::Url;

pub(super) struct MockResponse {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) headers: Vec<(&'static str, &'static str)>,
    pub(super) body: &'static str,
    pub(super) delay: Option<Duration>,
}

impl MockResponse {
    pub(super) const fn json(body: &'static str) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            headers: Vec::new(),
            body,
            delay: None,
        }
    }

    pub(super) const fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "application/json",
            headers: Vec::new(),
            body: "",
            delay: None,
        }
    }
}

pub(super) type Routes = BTreeMap<&'static str, Vec<MockResponse>>;

pub(super) async fn server(
    routes: Routes,
) -> Result<
    (
        Url,
        mpsc::UnboundedReceiver<String>,
        JoinHandle<Result<(), io::Error>>,
    ),
    Box<dyn std::error::Error>,
> {
    let request_count = routes.values().map(Vec::len).sum();
    let routes = routes
        .into_iter()
        .map(|(path, responses)| (path, responses.into_iter()))
        .collect::<BTreeMap<_, _>>();
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut routes = routes;
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().await?;
            let request = read_request(&mut stream).await?;
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing request path")
                })?;
            let response = routes
                .get_mut(path)
                .and_then(Iterator::next)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "unexpected request path")
                })?;
            let _ = request_tx.send(request);
            write_response(&mut stream, response).await?;
        }
        Ok(())
    });
    Ok((Url::parse(&format!("http://{address}/"))?, request_rx, task))
}

async fn read_request(stream: &mut TcpStream) -> Result<String, io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before headers",
            ));
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&request).into_owned());
        }
    }
}

async fn write_response(stream: &mut TcpStream, response: MockResponse) -> Result<(), io::Error> {
    if let Some(delay) = response.delay {
        tokio::time::sleep(delay).await;
    }
    let reason = match response.status {
        200 => "OK",
        302 => "Found",
        304 => "Not Modified",
        503 => "Service Unavailable",
        _ => "Response",
    };
    let mut headers = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.content_type,
        response.body.len(),
    );
    for (name, value) in response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(response.body.as_bytes()).await?;
    stream.shutdown().await
}
