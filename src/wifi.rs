use esp_idf_svc::{
    wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi},
    eventloop::EspSystemEventLoop,
};
use log;
use anyhow::{bail, Result, Ok};
use std::sync::{Arc, Mutex};

pub fn connect(
    wifi_mutex: Arc<Mutex<EspWifi>>,
    ssid: &str,
    psk: &str,
    sysloop: EspSystemEventLoop,
) -> Result<()> {

    let mut auth_method = AuthMethod::WPA2Personal;
    if ssid.is_empty() {
        bail!("No access point name");
    }

    if psk.is_empty() {
        auth_method = AuthMethod::None;
    }

    let mut wifi_locked = wifi_mutex.lock()
        .or_else(|e| {
            bail!("Wifi mutex is poisoned: {e}")
        })?;
    let mut wifi = BlockingWifi::wrap(&mut *wifi_locked, sysloop.clone())?;

    // First we need to scan to find the correct channel
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;

    log::info!("Starting WiFi");

    wifi.start()?;

    let ap_infos = wifi.scan()?;
    let channel = ap_infos.into_iter()
        .find(|a| a.ssid == ssid)
        .and_then(|a| Some(a.channel));

    // Reconfigure with correct info
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid.try_into().or_else(|_| {
            bail!("SSID {} couldn't be converted to heapless::String<32>", ssid)
        })?,
        password: psk.try_into().or_else(|_| {
            bail!("PSK couldn't be converted to heapless::String<64>")
        })?,
        channel,
        auth_method,
        ..Default::default()
    }))?;

    log::info!("Connecting WiFi");

    wifi.connect()?;

    wifi.wait_netif_up()?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;

    log::info!("Connected. DHCP info: {:?}", ip_info);

    Ok(())
}
