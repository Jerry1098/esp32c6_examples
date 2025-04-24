#![no_std]
#![no_main]

use core::net::Ipv6Addr;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, ConfigV6, Ipv6Cidr, Runner, StackResources, StaticConfigV6};
use embedded_io_async::Write;
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::systimer::SystemTimer;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

use esp_ieee802154::Ieee802154;
use heapless::Vec;
use openthread::{enet::{self, EnetDriver, EnetDriverState, EnetRunner}, esp::EspRadio, OpenThread, OtResources, OtRngCore, SimpleRamSettings};
use tinyrlibc as _;
extern crate alloc;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    thread_dataset: &'static str,
    #[default("")]
    mqtt_fqdn: &'static str,
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

const BOUND_PORT: u16 = 1212;

const IPV6_PACKET_SIZE: usize = 1280;
const ENET_MAX_SOCKETS: usize = 2;

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    info!("Starting...");

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
    let enet_driver_state = mk_static!(EnetDriverState<IPV6_PACKET_SIZE, 1, 1>, EnetDriverState::new());

    let ot_settings = mk_static!(SimpleRamSettings, SimpleRamSettings::new(ot_settings_buf));

    let ot = OpenThread::new(ieee_eui64, rng, ot_settings, ot_resources).unwrap();

    let (_enet_controller, enet_driver_runner, enet_driver) = enet::new(ot.clone(), enet_driver_state);


    spawner.spawn(run_enet_driver(
        enet_driver_runner,
        EspRadio::new(Ieee802154::new(peripherals.IEEE802154, peripherals.RADIO_CLK))
    )).unwrap();

    let enet_resources = mk_static!(StackResources<ENET_MAX_SOCKETS>, StackResources::new());

    let (stack, enet_runner) = embassy_net::new(enet_driver, embassy_net::Config::default(), enet_resources, enet_seed);

    spawner.spawn(run_enet(enet_runner)).unwrap();

    info!("Thread dataset: {:?}", app_config.thread_dataset);

    ot.set_active_dataset_tlv_hexstr(app_config.thread_dataset).unwrap();
    ot.enable_ipv6(true).unwrap();
    ot.enable_thread(true).unwrap();

    loop {
        info!("Waiting for IPv6 address from OpenThread...");
        
        let mut addrs = heapless::Vec::<(Ipv6Addr, u8), 4>::new();
        ot.ipv6_addrs(|addr| {
            if let Some(addr) = addr {
                let _ = addrs.push(addr);
            }

            Ok(())
        }).unwrap();

        if !addrs.is_empty() {
            info!("Got IPv6 addres(es) from OpenThread: {:?}", addrs);

            let (linklocal_addr, linklocal_prefix) = addrs
                .iter()
                .find(|(addr, _)| addr.is_unicast_link_local())
                .expect("No link-local address found");
            
            info!("Will bind to link-local {:?} Ipv6 addr", linklocal_addr);

            stack.set_config_v6(ConfigV6::Static(StaticConfigV6{
                address: Ipv6Cidr::new(*linklocal_addr, *linklocal_prefix),
                gateway: None, // TODO
                dns_servers: Vec::new(), // TODO
            }));

            break;
        }
    }

    spawner.spawn(run_ot_ip_info(ot.clone())).unwrap();

    // let (mut rx_meta, mut tx_meta) = ([Packet::EMPTY; 2], [PacketMetadata::EMPTY; 2]);
    // let rx_buf = unsafe { mk_static!([u8; IPV6_PACKET_SIZE]).assume_init_mut() };
    // let tx_buf = unsafe { mk_static!([u8; IPV6_PACKET_SIZE]).assume_init_mut() };

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut buf = [0; 4096];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    loop {

        info!("Listening on TCP:{:?}", BOUND_PORT);
        if let Err(e) = socket.accept(BOUND_PORT).await {
            warn!("accept error: {:?}", e);
            continue;
        }

        info!("Recieved connection from: {:?}", socket.remote_endpoint());

        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    warn!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!("{:?}", e);
                    break;
                }
            };
            info!("rxd {:?}", core::str::from_utf8(&buf[..n]).unwrap());

            if let Err(e) = socket.write_all(&buf[..n]).await {
                warn!("write error: {:?}", e);
                break;
            }

        }
    }
}


#[embassy_executor::task]
async fn run_enet_driver(mut runner: EnetRunner<'static, IPV6_PACKET_SIZE>, radio: EspRadio<'static>) -> ! {
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
