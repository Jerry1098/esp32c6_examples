#![no_std]
#![no_main]

use alloc::string::ToString;
use defmt::{debug, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use mqtt_led_relay::mqtt::create_mqtt_client;
use mqtt_led_relay::wifi::create_wifi_stack;


#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern crate alloc;

#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
    #[default("")]
    mqtt_fqdn: &'static str,
    #[default(0)]
    mqtt_port: u16,
    #[default("")]
    mqtt_username: &'static str,
    #[default("")]
    mqtt_password: &'static str,
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    info!("Starting up...");

    let app_config = CONFIG;

    info!("Config SSID: {}", app_config.wifi_ssid);
    info!("Config PSK: {}", app_config.wifi_psk);
    info!("Config MQTT FQDN: {}", app_config.mqtt_fqdn);
    info!("Config MQTT Port: {}", app_config.mqtt_port);
    info!("Config MQTT Username: {}", app_config.mqtt_username);    

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    info!("Initializing Wifi...");
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);
    let timer1 = TimerGroup::new(peripherals.TIMG0);

    let wifi_stack = match create_wifi_stack(
        app_config.wifi_ssid,
        app_config.wifi_psk,
        spawner,
        timer1,
        rng,
        peripherals.RADIO_CLK,
        peripherals.WIFI,
    ).await {
        Ok(wifi_stack) => wifi_stack,
        Err(e) => {
            info!("Error creating wifi stack: {:?}", e.to_string().as_str());
            return;
        }
    };

    create_mqtt_client(app_config.mqtt_username, app_config.mqtt_password, app_config.mqtt_fqdn, app_config.mqtt_port, "clientId-lkasd892", spawner, wifi_stack).await.unwrap();

    loop {
        rng.random();
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-beta.0/examples/src/bin
}
