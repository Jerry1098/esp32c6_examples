#![no_std]
#![no_main]

use core::net::{Ipv4Addr, Ipv6Addr};

use core::fmt::Write;
use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_net::{
    tcp::TcpSocket,
    ConfigV6, Ipv6Cidr, Runner, StackResources, StaticConfigV6,
};
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::systimer::SystemTimer;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

use esp_ieee802154::Ieee802154;
use heapless::{String, Vec};
use openthread::{
    enet::{self, EnetDriver, EnetDriverState, EnetRunner}, esp::EspRadio, DeviceRole, OpenThread, OtResources, OtRngCore, SimpleRamSettings
};
use rust_mqtt::{
    client::{client::MqttClient, client_config::ClientConfig},
    packet::v5::reason_codes::ReasonCode,
    utils::rng_generator::CountingRng,
};
use tinyrlibc as _;
extern crate alloc;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    thread_dataset: &'static str,
    #[default("")]
    mqtt_ip: &'static str,
    #[default(1883)]
    mqtt_port: u16,
    #[default("")]
    mqtt_username: &'static str,
    #[default("")]
    mqtt_password: &'static str,
}

macro_rules! mk_static {
    ($t:ty) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit();
        x
    }};
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

const IPV6_PACKET_SIZE: usize = 1280;
const ENET_MAX_SOCKETS: usize = 2;

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    info!("Starting...");

    let nat64_prefix = Ipv6Addr::new(0xfdb4, 0x4e7f, 0x4e8d, 0x2, 0, 0, 0, 0);
    let net64_prefix_length = 96;

    // Prefixes:
    // fd42:4696:c9c:1::/64 paos low a800
    // Routes:
    // fdb4:4e7f:4e8d:2:0:0::/96 
    // fc00::/7 s med a800

    // Goal: fd42:4696:c9c:2:0:0:c0a8:1e4 for 192.168.1.228
    // Got:  fd42:1258:0c9c:0002:0000:0000:c0a8:01e4
    // let prefix = Ipv6Addr::new(0xfdb4, 0x4e7f, 0x4e8d, 0x2, 0, 0, 0, 0);
    // let prefix_len = 96;
    // let prefix = Ipv6Addr::new(0xfd42, 4696, 0xc9c, 1, 0, 0, 0, 0);
    // let prefix_len = 64;
    // let ipv4 = Ipv4Addr::new(192, 168, 1, 228);

    // let net64_ipv6 = synthesize_nat64(prefix, prefix_len, ipv4);

    // info!("Synthesized IPv6 address: {:?}", net64_ipv6);

    let app_config = CONFIG;

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    let rng = mk_static!(Rng, Rng::new(peripherals.RNG));

    let enet_seed = rng.next_u64();

    let mut ieee_eui64 = [0; 8];
    rng.fill_bytes(&mut ieee_eui64);

    let ot_resources = mk_static!(OtResources, OtResources::new());
    let ot_settings_buf = mk_static!([u8; 1024], [0; 1024]);
    let enet_driver_state =
        mk_static!(EnetDriverState<IPV6_PACKET_SIZE, 1, 1>, EnetDriverState::new());

    let ot_settings = mk_static!(SimpleRamSettings, SimpleRamSettings::new(ot_settings_buf));

    let ot = OpenThread::new(ieee_eui64, rng, ot_settings, ot_resources).unwrap();

    let (_enet_controller, enet_driver_runner, enet_driver) =
        enet::new(ot.clone(), enet_driver_state);

    spawner
        .spawn(run_enet_driver(
            enet_driver_runner,
            EspRadio::new(Ieee802154::new(
                peripherals.IEEE802154,
                peripherals.RADIO_CLK,
            )),
        ))
        .unwrap();

    let enet_resources = mk_static!(StackResources<ENET_MAX_SOCKETS>, StackResources::new());

    let (stack, enet_runner) = embassy_net::new(
        enet_driver,
        embassy_net::Config::default(),
        enet_resources,
        enet_seed,
    );

    spawner.spawn(run_enet(enet_runner)).unwrap();

    info!("Thread dataset: {:?}", app_config.thread_dataset);

    ot.set_active_dataset_tlv_hexstr(app_config.thread_dataset)
        .unwrap();
    ot.enable_ipv6(true).unwrap();
    ot.enable_thread(true).unwrap();

    info!("Waiting for child role");
    loop {
        ot.wait_changed().await;

        if ot.net_status().role == DeviceRole::Child {
            info!("OT -> Role: Child");
            break;
        }
    }

    loop {
        info!("Waiting for IPv6 address from OpenThread...");

        let mut addrs = heapless::Vec::<(Ipv6Addr, u8), 4>::new();
        ot.ipv6_addrs(|addr| {
            if let Some(addr) = addr {
                let _ = addrs.push(addr);
            }

            Ok(())
        })
        .unwrap();

        if !addrs.is_empty() {
            info!("Got IPv6 addres(es) from OpenThread: {:?}", addrs);

            let (linklocal_addr, linklocal_prefix) = addrs
                .iter()
                .find(|(addr, _)| addr.segments()[0] == 0xfdb4) // .is_unicast_link_local()) //segments()[0] == 0xfdeb)
                .expect("No link-local address found");

            info!("Will bind to link-local {:?} Ipv6 addr", linklocal_addr);
            info!(
                "Will bind to link-local {:?} Ipv6 prefix",
                linklocal_prefix
            );

            stack.set_config_v6(ConfigV6::Static(StaticConfigV6 {
                address: Ipv6Cidr::new(*linklocal_addr, *linklocal_prefix),
                gateway: None,           // TODO (not needed asof now any trafic outside of thread needs to use nat64 addresses that need to be self generated) should be the otbr address?
                dns_servers: Vec::new(), // TODO (can be any address using nat64 synthesis)
            }));

            break;
        }
    }

    spawner.spawn(run_ot_ip_info(ot.clone())).unwrap();

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    loop {
        Timer::after(Duration::from_secs(1)).await;

        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(15)));

        // let mqtt_ip = core::net::Ipv4Addr::new(192, 168, 1, 228);
        // let mqtt_ip = core::net::Ipv4Addr::new(1,1,1,1);
        let mqtt_ip = synthesize_nat64(nat64_prefix, net64_prefix_length, app_config.mqtt_ip.parse::<Ipv4Addr>().unwrap());
        info!("Synthesized MQTT-Broker IPv6 address: {:?}", mqtt_ip);
        let mqtt_endpoint = (mqtt_ip, app_config.mqtt_port);
        info!("Connection to MQTT-Broker on {:?}", mqtt_endpoint);
        let connection = socket.connect(mqtt_endpoint).await;

        if let Err(e) = connection {
            error!("Connection error: {:?}", e);
            continue;
        }

        info!("Connected");

        let mut mqtt_client_config = ClientConfig::new(
            rust_mqtt::client::client_config::MqttVersion::MQTTv5,
            CountingRng(20000),
        );
        mqtt_client_config.add_username(app_config.mqtt_username);
        mqtt_client_config.add_password(app_config.mqtt_password);
        mqtt_client_config.add_client_id("clientId-2m3km334gd");
        mqtt_client_config.max_packet_size = 100;
        let mut recv_buffer = [0; 80];
        let mut write_buffer = [0; 80];

        let mut mqtt_client = MqttClient::<_, 5, _>::new(
            socket,
            &mut write_buffer,
            80,
            &mut recv_buffer,
            80,
            mqtt_client_config,
        );

        match mqtt_client.connect_to_broker().await {
            Ok(()) => {}
            Err(mqtt_error) => match mqtt_error {
                ReasonCode::NetworkError => {
                    error!("MQTT Network error");
                    continue;
                }

                _ => {
                    error!("Other MQTT Error: {:?}", mqtt_error);
                    continue;
                }
            },
        }
        let random_number = 123456;
        info!("Sending number: {}", random_number);

        let mut number_string: String<32> = String::new();
        write!(number_string, "{:.2}", random_number).expect("write! failed");

        match mqtt_client
            .send_message(
                "random/1",
                number_string.as_bytes(),
                rust_mqtt::packet::v5::publish_packet::QualityOfService::QoS1,
                true,
            )
            .await
        {
            Ok(()) => {}
            Err(mqtt_error) => match mqtt_error {
                ReasonCode::NetworkError => {
                    error!("MQTT NEtwork error")
                }
                _ => {
                    error!("Other MQTT error: {:?}", mqtt_error)
                }
            },
        }

        Timer::after(Duration::from_secs(5)).await
    }
}

#[embassy_executor::task]
async fn run_enet_driver(
    mut runner: EnetRunner<'static, IPV6_PACKET_SIZE>,
    radio: EspRadio<'static>,
) -> ! {
    runner.run(radio).await
}

#[embassy_executor::task]
async fn run_enet(mut runner: Runner<'static, EnetDriver<'static, IPV6_PACKET_SIZE>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn run_ot_ip_info(ot: OpenThread<'static>) -> ! {
    let mut curr_addrs = heapless::Vec::<(Ipv6Addr, u8), 4>::new();

    loop {
        let mut addrs = heapless::Vec::<(Ipv6Addr, u8), 4>::new();
        ot.ipv6_addrs(|addr| {
            if let Some(addr) = addr {
                let _ = addrs.push(addr);
            }

            Ok(())
        })
        .unwrap();

        if curr_addrs != addrs {
            info!("Got new IPv6 address(es) from OpenThread: {:?}", addrs);

            curr_addrs = addrs;

            info!("Waiting for OpenThread changes signal...");
        }

        ot.wait_changed().await;
    }
}


/// This function mimics the NAT64 synthesis function Address::SynthesizeFromIp4Address
/// from the OpenThread codebase.
fn synthesize_nat64(prefix: Ipv6Addr, prefix_len: u8, ipv4: Ipv4Addr) -> Ipv6Addr {
    
    assert!([32, 40, 48, 56, 64, 96].contains(&prefix_len), "Invalid prefix length");

    let mut ipv6_bytes = prefix.octets();
    let ipv4_bytes = ipv4.octets();

    let skip_index = 8; // kSkipIndex
    let mut ip6_index = (prefix_len / 8) as usize; // aPrefix.GetLength() / kBitsPerByte;

    for &b in ipv4_bytes.iter() {
        if ip6_index == skip_index {
            ip6_index += 1;
        }
        if ip6_index >= 16 {
            break;
        }
        ipv6_bytes[ip6_index] = b;
        ip6_index += 1;
    }

    Ipv6Addr::from(ipv6_bytes)
}