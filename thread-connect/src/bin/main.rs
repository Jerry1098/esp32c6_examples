#![no_std]
#![no_main]

use core::net::{Ipv6Addr, SocketAddrV6};

use defmt::info;
use dotenvy_macro::dotenv;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::rng::Rng;
use esp_hal::timer::systimer::SystemTimer;
use openthread::esp::{EspRadio, Ieee802154};
use openthread::{
    BytesFmt, OpenThread, OtResources, OtRngCore, OtUdpResources, SimpleRamSettings, UdpSocket
};
use panic_rtt_target as _;

use tinyrlibc as _;

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

const UDP_SOCKETS_BUF: usize = 1280;
const UDP_MAX_SOCKETS: usize = 2;

const THREAD_DATASET: &str = dotenv!("THREAD_DATASET");

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    rtt_target::rtt_init_defmt!();

    info!("Starting...");

    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_hal_embassy::init(SystemTimer::new(peripherals.SYSTIMER).alarm0);

    info!("Embassy initialized!");

    let rng = mk_static!(Rng, Rng::new(peripherals.RNG));

    let mut ieee_euid64 = [0; 8];
    rng.fill_bytes(&mut ieee_euid64);

    let ot_resources = mk_static!(OtResources, OtResources::new());
    let ot_udp_resources =
        mk_static!(OtUdpResources<UDP_MAX_SOCKETS, UDP_SOCKETS_BUF>, OtUdpResources::new());
    let ot_settings_buf = mk_static!([u8; 1024], [0; 1024]);

    let ot_settings = mk_static!(SimpleRamSettings, SimpleRamSettings::new(ot_settings_buf));

    let ot = OpenThread::new_with_udp(
        ieee_euid64,
        rng,
        ot_settings,
        ot_resources,
        ot_udp_resources,
    )
    .unwrap();

    spawner
        .spawn(run_ot(
            ot.clone(),
            EspRadio::new(Ieee802154::new(
                peripherals.IEEE802154,
                peripherals.RADIO_CLK,
            )),
        ))
        .unwrap();

    spawner.spawn(run_ot_ip_info(ot.clone())).unwrap();

    info!("Dataset: {}", THREAD_DATASET);

    ot.set_active_dataset_tlv_hexstr(THREAD_DATASET).unwrap();
    ot.enable_ipv6(true).unwrap();
    ot.enable_thread(true).unwrap();

    let socket = UdpSocket::bind(
        ot,
        &SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, BOUND_PORT, 0, 0),
    )
    .unwrap();

    info!(
        "Opened socket on port {} and waiting for packets...",
        BOUND_PORT
    );

    let buf: &mut [u8] = unsafe { mk_static!([u8; UDP_SOCKETS_BUF]).assume_init_mut() };

    loop {
        let (len, local, remote) = socket.recv(buf).await.unwrap();

        info!("Got {} from {} on {}", BytesFmt(&buf[..len]), remote, local);

        socket.send(b"Hello", Some(&local), &remote).await.unwrap();
        info!("Sent `b\"Hello\"`");
    }
}

#[embassy_executor::task]
async fn run_ot(ot: OpenThread<'static>, radio: EspRadio<'static>) -> ! {
    ot.run(radio).await
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
