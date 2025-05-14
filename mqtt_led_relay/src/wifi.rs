use core::fmt::Display;

use anyhow::{Error, Result};
use defmt::{debug, error, info};
use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::{
    peripherals::{RADIO_CLK, TIMG0, WIFI},
    rng::Rng,
    timer::timg::TimerGroup,
};
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState},
    EspWifiController, InitializationError,
};

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

pub async fn create_wifi_stack(
    wifi_ssid: &'static str,
    wifi_psk: &'static str,
    spawner: Spawner,
    timer: TimerGroup<TIMG0>,
    mut rng: Rng,
    radio_clk: RADIO_CLK,
    wifi: WIFI,
) -> Result<Stack<'static>> {
    let esp_wifi_controller: &EspWifiController<'static> = mk_static!(
        EspWifiController<'static>,
        init(timer.timer0, rng, radio_clk).map_err(|e| Error::msg(WrappedInitError(e)))?
    );

    let (controller, interface) = esp_wifi::wifi::new(&esp_wifi_controller, wifi)
        .map_err(| e | { Error::msg(WrappedWifiError(e)) })?;
    let wifi_interface = interface.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(controller, wifi_ssid, wifi_psk))?;
    spawner.spawn(net_task(runner))?;

    debug!("Wifi stack initialized");

    debug!("Waiting for Link Up ...");
    // wait until wifi is connected
    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(100)).await;
    }
    debug!("Link Up!");

    info!("Waiting for IP address ...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP address: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(100)).await;
    }

    Ok(stack)
}

#[embassy_executor::task]
async fn connection(
    mut controller: WifiController<'static>,
    ssid: &'static str,
    psk: &'static str,
) {
    info!("Connection Task started");

    match controller.capabilities() {
        Ok(capabilities) => {
            for capability in capabilities {
                debug!("Capability: {:?}", capability);
            }
        }
        Err(e) => {
            error!("Failed to get device capabilities: {:?}", e);
            return;
        }
    }

    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                info!("Disconnected from AP, waiting for 5 seconds ...");
                Timer::after(Duration::from_secs(5)).await;
            }
            _ => {}
        }

        if !matches!(controller.is_started(), Ok(true)) {
            info!("Trying to connect to {:?} with password {:?}", ssid, psk);
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: ssid.try_into().unwrap(),
                password: psk.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("Starting WiFi controller...");
            controller.start_async().await.unwrap();
            info!("Wifi controller started");
        }

        info!("Connecting to AP...");

        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected"),
            Err(e) => {
                error!("Failed to connect to AP: {:?}", e);
                Timer::after(Duration::from_secs(5)).await;
                info!("Retrying connection...");
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    debug!("Net task started");
    runner.run().await;
}

// Wrapper for Error types to work with anyhow

#[derive(Debug, defmt::Format)]
pub struct WrappedInitError(InitializationError);

impl Display for WrappedInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Wifi initialization error: {:?}", self.0)
    }
}

#[derive(Debug, defmt::Format)]
pub struct WrappedWifiError(esp_wifi::wifi::WifiError);

impl Display for WrappedWifiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Wifi error: {:?}", self.0)
    }
}
