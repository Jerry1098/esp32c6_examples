#![no_std]
#![no_main]

// Used Source: https://github.com/JurajSadel/esp32c3-no-std-async-mqtt-demo


use defmt::{debug, error, info};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Runner, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use esp_wifi::wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState};
use esp_wifi::{init, EspWifiController};
use heapless::String;
use panic_rtt_target as _;
use core::fmt::Write;
use rust_mqtt::client::client::MqttClient;
use rust_mqtt::client::client_config::ClientConfig;
use rust_mqtt::packet::v5::reason_codes::ReasonCode;
use rust_mqtt::utils::rng_generator::CountingRng;
use smoltcp::wire::DnsQueryType;

extern crate alloc;

const SSID: &str = "Neuland";
const PASSWORD: &str = "GMKspY7A8brqw63y";

const MQTT_FQDN: &str = "homeassistant";
const MQTT_PORT: u16 = 1883;

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }}
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {

    rtt_target::rtt_init_defmt!();

    info!("Hello World");

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    let timer1 = TimerGroup::new(peripherals.TIMG0);
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let esp_wifi_controller: &EspWifiController<'static> = &*mk_static!(EspWifiController<'static>, init(timer1.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap());

    let (controller, interface) = esp_wifi::wifi::new(&esp_wifi_controller, peripherals.WIFI).unwrap();
    let wifi_interface = interface.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(wifi_interface, config, mk_static!(StackResources<3>, StackResources::<3>::new()), seed);


    spawner.spawn(connection(controller)).ok();
    spawner.spawn(net_task(runner)).ok();

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    // wait until wifi is connected
    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    info!("Waiting for IP address...");

    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    loop {
        Timer::after(Duration::from_secs(1)).await;

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

        socket.set_timeout(Some(embassy_time::Duration::from_secs(15)));

        let mqtt_broker_ip_address = match stack
            .dns_query(MQTT_FQDN, DnsQueryType::A)
            .await
            .map(|a| a[0])
        {
            Ok(address) => address,
            Err(e) => {
                error!("DNS lookup error: {:?}", e);
                continue;
            }
        };

        let mqtt_endpoint = (mqtt_broker_ip_address, MQTT_PORT);
        info!("Connecting...");
        let connection = socket.connect(mqtt_endpoint).await;

        if let Err(e) = connection {
            error!("Connection error: {:?}", e);
            continue;
        }
        info!("Connected!");

        let mut config = ClientConfig::new(rust_mqtt::client::client_config::MqttVersion::MQTTv5, CountingRng(20000));
        config.add_client_id("clientId-2m3km334gd");
        config.max_packet_size = 100;
        let mut recv_buffer = [0; 80];
        let mut write_buffer = [0; 80];

        let mut client =
            MqttClient::<_, 5, _>::new(socket, &mut write_buffer, 80, &mut recv_buffer, 80, config);
        
        match client.connect_to_broker().await {
            Ok(()) => {}
            Err(mqtt_error) => match mqtt_error {
                ReasonCode::NetworkError => {
                    error!("MQTT Network Error");
                    continue;
                }
                _ => {
                    error!("Other MQTT Error: {:?}", mqtt_error);
                    continue;
                }
            }
        }

        loop {
            let random_number = rng.random();
            info!("Sening number: {}", random_number);

            let mut number_string: String<32> = String::new();
            write!(number_string, "{:.2}", random_number).expect("write! failed!");

            match client.send_message("random/1", number_string.as_bytes(), rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1, true).await {
                Ok(()) => {}
                Err(mqtt_error) => match mqtt_error {
                    ReasonCode::NetworkError => {
                        error!("MQTT Network Error");
                        continue;
                    }
                    _ => {
                        error!("Other MQTT Error: {:?}", mqtt_error);
                        continue;
                    }
                },
            }

            Timer::after(Duration::from_secs(5)).await;
        }

    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-beta.0/examples/src/bin
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    info!("start connection task");
    debug!("Device capabilities: {:?}", controller.capabilities());

    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("Starting Wifi...");
            controller.start_async().await.unwrap();
            info!("Wifi started!");
        }
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect to wifi: {:?}", e);
                Timer::after(Duration::from_millis(5_000)).await
            }
        }
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await;
}