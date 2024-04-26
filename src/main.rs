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
use lsm9ds1::{LSM9DS1Init, LSM9DS1};
use std::time::Duration;
use std::sync::{Arc, Mutex};
use hecate_protobuf as proto;
use proto::{Message, SensorDataSample};
use ringbuffer::{RingBuffer, AllocRingBuffer};

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

    sensor.begin_accel().expect("Failed to initialize accelerometer");
    sensor.begin_gyro().expect("Failed to initialize gyroscope");
    sensor.begin_mag().expect("Failed to initialize magnetometer");

    // Create ringbuffer for sensor data
    let sensor_data_ringbuffer = AllocRingBuffer::<proto::SensorDataSample>::new(128);
    let sensor_data_ringbuffer_mutex = Arc::new(Mutex::new(sensor_data_ringbuffer));

    // Spawn sensor sampling thread
    let mutex_clone = sensor_data_ringbuffer_mutex.clone();
    std::thread::Builder::new()
        .name("sensor sampling thread".into())
        .spawn(move ||
            sensor_sampling_thread(sensor, mutex_clone)
                .inspect_err(|e| log::error!("Sensor sampling thread died: {e}"))
        ).expect("Failed to create sensor sampling thread");

        
    // Setup networking
    let wifi = EspWifi::new(p.modem, sysloop.clone(), Some(nvs.clone()))?;    
    let wifi_mutex = Arc::new(Mutex::new(wifi));
    
    let wifi_mutex_clone = wifi_mutex.clone();
    let sysloop_clone = sysloop.clone();
    let buffer_mutex_clone = sensor_data_ringbuffer_mutex.clone();
    std::thread::Builder::new()
        .name("networking thread".into())
        .stack_size(16384)
        .spawn(move ||
            networking_thread(wifi_mutex_clone, sysloop_clone, buffer_mutex_clone)
                .inspect_err(|e| log::error!("Networking thread died: {e}"))
        ).expect("Failed to create networking thread");
    
    // Start WiFi indicator led
    let indicator_led = PinDriver::output(p.pins.gpio13)?;
    let wifi_mutex_clone = wifi_mutex.clone();
    std::thread::Builder::new()
        .name("WiFi indicator".into())
        .spawn(move ||
            wifi_indicator(indicator_led, wifi_mutex_clone)
                .inspect_err(|e| log::error!("WiFi Indicator died (;ω;) ({e})"))
        ).expect("Failed to create wifi indicator thread");

    
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn networking_thread<R>(wifi_mutex: Arc<Mutex<EspWifi>>, sysloop: EspSystemEventLoop, data_buffer: Arc<Mutex<R>>) -> Result<()>
where
    R: RingBuffer<proto::SensorDataSample>,
{
    
    // Connect to WiFi
    log::info!("Connecting to WiFi {}", CONFIG.wifi_ssid);
    wifi::connect(wifi_mutex.clone(), CONFIG.wifi_ssid, CONFIG.wifi_psk, sysloop.clone())
        .inspect_err(|e| log::error!("Error during WiFi connection attempt: {}", e))?;
    log::info!("Connected");

    // Open WS connection
    log::info!("Connecting to {}:{}{}", CONFIG.ws_host, CONFIG.ws_port, CONFIG.ws_endpoint);
    let mut client = Box::new(ws::WebSocketClient::<4096>::new());
    client.connect(CONFIG.ws_host, CONFIG.ws_port, CONFIG.ws_endpoint)
        .expect("Websocket client failed to connect");
    log::info!("Connected");

    // Send ID as text
    client.send_text("Feather")?;

    loop {
        let samples = data_buffer.lock()
            .inspect_err(|e| log::error!("Failed to lock ringbuffer mutex: {}", e))
            .ok()
            .and_then(|mut buffer| Some(buffer.drain().take(100).collect::<Vec<_>>()));

        if let Some(samples) = samples {
            let message = proto::SensorData {
                samples,
            }.encode_to_vec();

            _ = client.send_binary(&message)
                .inspect_err(|e| log::error!("Failed to send data: {}", e))
                .and_then(|_| { log::info!("Sent data"); Ok(())});
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

fn sensor_sampling_thread<I, R>(mut sensor: LSM9DS1<I>, buffer_mutex: Arc<Mutex<R>>) -> Result<()>
where
    I: lsm9ds1::interface::Interface,
    R: RingBuffer<SensorDataSample>,
{
    let timer = EspTimerService::new().expect("Failed to initialize timer service");
    let start_time = timer.now();

    loop {
        let acc = sensor.read_accel();
        let gyro = sensor.read_gyro();
        let mag = sensor.read_mag();

        if let (Ok((ax, ay, az)), Ok((gx, gy, gz)), Ok((mx, my, mz))) = (acc, gyro, mag) {
            let time = timer.now() - start_time;
            let sample = proto::SensorDataSample {
                time: time.as_secs_f32(),
                acceleration: proto::Acceleration{ x: ax, y: ay, z: az },
                magnetometer: proto::MagnetometerData { x: mx, y: my, z: mz },
                gyroscope: proto::GyroscopeData { x: gx, y: gy, z: gz },
            };

            match buffer_mutex.lock() {
                Ok(mut buffer) => buffer.push(sample),
                Err(e) => log::error!("Error locking ringbuffer mutex: {}", e),
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
