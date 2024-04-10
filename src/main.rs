use anyhow::Result;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        self,
        gpio::{PinDriver, Output, OutputPin},
    },
    nvs::EspDefaultNvsPartition,
    wifi::EspWifi,
};
use embedded_websocket as ws;
use std::net::TcpStream;
use std::time::Duration;
use std::sync::{Arc, Mutex};

mod wifi;

#[toml_cfg::toml_config]
struct Config {
    #[default("Free Wi-Fi")]
    wifi_ssid: &'static str,
    #[default("BiBiBiBiBi")]
    wifi_psk: &'static str,
    #[default("echo.websocket.org")]
    ws_host: &'static str,
    #[default("8000")]
    ws_port: &'static str,
    #[default("/")]
    ws_endpoint: &'static str,
}

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    // Get peripherals
    let p = hal::peripherals::Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // Setup WiFi
    let wifi = EspWifi::new(p.modem, sysloop.clone(), Some(nvs.clone()))?;    
    let wifi_mutex = Arc::new(Mutex::new(wifi));
    
    // Start WiFi indicator led
    let indicator_led = PinDriver::output(p.pins.gpio13)?;
    let wifi_mutex_clone = wifi_mutex.clone();
    std::thread::spawn(move || wifi_indicator(indicator_led, wifi_mutex_clone).map_err(|e| {
        log::error!("WiFi Indicator died (;ω;) ({e})");
        anyhow::Ok(())
    }));

    // Connect to WiFi
    log::info!("Connecting to {}", CONFIG.wifi_ssid);
    wifi::connect(wifi_mutex.clone(), CONFIG.wifi_ssid, CONFIG.wifi_psk, sysloop.clone())?;

    // Open WS connection
    let ws_host = format!("{}:{}", CONFIG.ws_host, CONFIG.ws_port);
    let mut stream = TcpStream::connect(&ws_host).expect("Failed to open TCP connection");
    let mut read_buf: [u8; 2048] = [0; 2048];
    let mut write_buf: [u8; 2048] = [0; 2048];
    let mut read_cursor = 0;
    let mut websocket = ws::WebSocketClient::new_client(rand::thread_rng());
    let ws_options = ws::WebSocketOptions {
        path: CONFIG.ws_endpoint,
        host: CONFIG.ws_host,
        origin: &ws_host,
        sub_protocols: None,
        additional_headers: None,
    };
    let mut framer = ws::framer::Framer::new(&mut read_buf, &mut read_cursor, &mut write_buf, &mut websocket);
    framer.connect(&mut stream, &ws_options)
        .expect("Failed to connect framer");

    log::info!("Connected");

    let message: [u8; 3] = [48,49,50];
    framer.write(&mut stream, ws::WebSocketSendMessageType::Binary, true, &message)
        .expect("Failed to send message");

    framer.close(&mut stream, ws::WebSocketCloseStatusCode::NormalClosure, None)
        .expect("Error during close");

    anyhow::Ok(())
}

fn wifi_indicator<P>(mut led: PinDriver<P, Output>, wifi_mutex: Arc<Mutex<EspWifi>>) -> Result<()>
where
    P: OutputPin {

    loop {

        let is_up = wifi_mutex.lock().ok()
            .and_then(|wifi| wifi.is_up().ok())
            .unwrap_or(false);
        _ = led.set_level(is_up.into());

        std::thread::sleep(Duration::from_millis(200));
    }
}
