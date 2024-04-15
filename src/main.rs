use anyhow::Result;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        self,
        gpio::{PinDriver, Output, OutputPin},
        i2c::I2cDriver,
    },
    nvs::EspDefaultNvsPartition,
    wifi::EspWifi,
    timer::EspTimerService,
};
use lsm9ds1::LSM9DS1Init;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use hecate_protobuf as proto;
use proto::{Acceleration, Message};

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

    // Power pin setup
    // QWIIC connector power pin is driven by a regulator controlled by the same
    // pin as power to the neopixel.
    let mut power = PinDriver::output(p.pins.gpio2)?;
    power.set_high()?;
    std::thread::sleep(Duration::from_millis(20)); // Sensor needs some time for proper power-up

    // Sensor setup
    let ag_addr = lsm9ds1::interface::i2c::AgAddress::_2;
    let mag_addr = lsm9ds1::interface::i2c::MagAddress::_2;
    let sensor_i2c = I2cDriver::new(p.i2c0, p.pins.gpio22, p.pins.gpio20, &Default::default()).unwrap();
    let sensor_interface = lsm9ds1::interface::I2cInterface::init(sensor_i2c, ag_addr, mag_addr);
    let mut sensor = LSM9DS1Init::default().with_interface(sensor_interface);

    sensor.begin_accel().expect("Failed to initialize sensor");

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
    let mut client = Box::new(ws::WebsocketClient::<4096>::new());
    client.connect(CONFIG.ws_host, CONFIG.ws_port, CONFIG.ws_endpoint)
        .expect("Websocket client failed to connect");

    log::info!("Connected");

    let mut samples = Vec::<proto::SensorDataSample>::new();
    let timer = EspTimerService::new().expect("Failed to initialize timer service");
    let start_time = timer.now();
    loop {

        if let Ok((x, y, z)) = sensor.read_accel() {
            let time = timer.now() - start_time;
            let sample = proto::SensorDataSample {
                time: time.as_secs_f32(),
                acceleration: Acceleration { x, y, z },
            };
            samples.push(sample);
        }

        if samples.len() >= 100 {
            let message = proto::SensorData {
                id: "feather".into(),
                samples: samples,
            }.encode_to_vec();

            match client.send_binary(&message) {
                Ok(_) => { log::info!("Sent data"); },
                Err(e) => { log::error!("Failed to send data: {}", e); },
            }

            samples = Vec::new();
        }

        std::thread::sleep(Duration::from_millis(100));
    }
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
