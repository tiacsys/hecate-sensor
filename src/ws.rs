use std::net::TcpStream;
use std::error::Error;
use std::fmt::Display;
use rand::{
    self,
    rngs::ThreadRng,
};
use embedded_websocket as ews;
use ews::{framer::Framer, WebSocketOptions};

pub struct WebsocketClient<'a, const BUFSIZE: usize> {
    tcp_stream: Option<TcpStream>,
    websocket: ews::WebSocketClient<ThreadRng>,
    ws_options: ews::WebSocketOptions<'a>,
    read_buf: [u8; BUFSIZE],
    write_buf: [u8; BUFSIZE],
    read_cursor: usize,
}

#[derive(Debug)]
pub enum WebSocketClientError {
    TcpError,
    WebSocketError,
    NotConnected,
}

impl Display for WebSocketClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for WebSocketClientError {}

impl<'a, const BUFSIZE: usize> WebsocketClient<'a, BUFSIZE> {
    pub fn new() -> Self {
        let read_buf = [0; BUFSIZE];
        let write_buf = [0; BUFSIZE];
        let read_cursor = 0;
        let websocket = ews::WebSocketClient::new_client(rand::thread_rng());
        let ws_options = WebSocketOptions {
            path: "",
            host: "",
            origin: "",
            sub_protocols: None,
            additional_headers: None,
        };

        Self {
            tcp_stream: None,
            websocket,
            ws_options,
            read_buf,
            write_buf,
            read_cursor,
        }
    }
    
    pub fn connect(&mut self, host: &'a str, port: u16, endpoint: &'a str) -> Result<(), WebSocketClientError> {
        
        let host_port = format!("{}:{}", host, port);
        let mut tcp_stream = TcpStream::connect(host_port)
            .map_err(|_| WebSocketClientError::TcpError)?;
        
        let mut framer = Framer::new(&mut self.read_buf, &mut self.read_cursor, &mut self.write_buf, &mut self.websocket);
        
        let ws_options = WebSocketOptions {
            path: endpoint,
            host: host,
            origin: host,
            ..self.ws_options
        };

        framer.connect(&mut tcp_stream, &ws_options)
            .map_err(|_| WebSocketClientError::WebSocketError)?;
    
        self.ws_options = ws_options;
        self.tcp_stream = Some(tcp_stream);

        Ok(())
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), WebSocketClientError> {

        match self.tcp_stream.as_mut() {
            None => Err(WebSocketClientError::NotConnected),
            Some(tcp_stream) => {
                let mut framer = Framer::new(&mut self.read_buf, &mut self.read_cursor, &mut self.write_buf, &mut self.websocket);
                framer.write(tcp_stream, ews::WebSocketSendMessageType::Text, true, text.as_bytes())
                    .map_err(|_| WebSocketClientError::WebSocketError)?;
                Ok(())
            }
        }
    }

    pub fn send_binary(&mut self, buf: &[u8]) -> Result<(), WebSocketClientError> {

        match self.tcp_stream.as_mut() {
            None => Err(WebSocketClientError::NotConnected),
            Some(tcp_stream) => {
                let mut framer = Framer::new(&mut self.read_buf, &mut self.read_cursor, &mut self.write_buf, &mut self.websocket);
                framer.write(tcp_stream, ews::WebSocketSendMessageType::Binary, true, buf)
                    .map_err(|_| WebSocketClientError::WebSocketError)?;
                Ok(())
            }
        }
    }
}
