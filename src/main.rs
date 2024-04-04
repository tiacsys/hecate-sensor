use anyhow::{bail, Result, Ok, Error};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        self,
        gpio::{PinDriver, Output, OutputPin},
    },
    nvs::EspDefaultNvsPartition,
    wifi::EspWifi,
    ping::{self, EspPing},
    ipv4::Ipv4Addr,
};
use std::time::Duration;
use std::sync::{Arc, Mutex};

mod wifi;

#[toml_cfg::toml_config]
struct Config {
    #[default("Free Wi-Fi")]
    wifi_ssid: &'static str,
    #[default("BiBiBiBiBi")]
    wifi_psk: &'static str,
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
        Ok(())
    }));

    // Connect to WiFi
    wifi::connect(wifi_mutex.clone(), CONFIG.wifi_ssid, CONFIG.wifi_psk, sysloop.clone())?;

    let mut ping = EspPing::new(0);
    let summary = ping.ping(Ipv4Addr::new(9, 9, 9, 9), &ping::Configuration {
        count: 4,
        ..Default::default()
    })?;

    log::info!("Ping summary: {:?}", summary);

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
