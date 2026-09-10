use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread::{self, JoinHandle},
    time::Duration,
};

/// A deliberately tiny HTTP server for verifying the requests sent to a local
/// Ollama-compatible endpoint. It accepts one request and then closes.
pub struct MockHttpServer {
    host: String,
    request: JoinHandle<String>,
}

impl MockHttpServer {
    pub fn respond_once(status: u16, body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock HTTP server");
        let host = listener.local_addr().expect("mock server address").to_string();
        let body = body.to_string();
        let request = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock HTTP request");
            let request = read_request(&mut stream).expect("read mock HTTP request");
            let response = format!(
                "HTTP/1.1 {status} Test Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write mock HTTP response");
            request
        });

        Self { host, request }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn finish(self) -> String {
        self.request.join().expect("mock HTTP server panicked")
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    let mut expected_length = None;

    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);

        if expected_length.is_none() {
            if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
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
                expected_length = Some(header_end + 4 + content_length);
            }
        }

        if expected_length.is_some_and(|length| request.len() >= length) {
            break;
        }
    }

    Ok(String::from_utf8_lossy(&request).to_string())
}
