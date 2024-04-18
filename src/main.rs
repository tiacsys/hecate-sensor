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
use std::time::Duration;
use std::sync::{Arc, Mutex};
use hecate_protobuf as proto;
use proto::Message;

mod wifi;
mod ws;

#[toml_cfg::toml_config]
struct Config {
    #[default("Free Wi-Fi")]
    wifi_ssid: &'static str,
    #[default("BiBiBiBiBi")]
    wifi_psk: &'static str,
    #[default("echo.websocket.org")]
    ws_host: &'static str,
    #[default(8000)]
    ws_port: u16,
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
    let mut client = Box::new(ws::WebsocketClient::<2048>::new());
    client.connect(CONFIG.ws_host, CONFIG.ws_port, CONFIG.ws_endpoint)
        .expect("Websocket client failed to connect");

    log::info!("Connected");

    loop {
        let message = proto::Acceleration {
            x: 1.1,
            y: 2.2,
            z: 3.3,
        };
        let buffer = message.encode_to_vec();

        client.send_binary(&buffer)?;

        std::thread::sleep(Duration::from_secs(1));
    }

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
