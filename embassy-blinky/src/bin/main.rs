#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{gpio::GpioPin, rmt::Rmt, time::Rate, timer::systimer::SystemTimer};
use esp_hal_smartled::{smartLedBuffer, SmartLedsAdapter};
use panic_rtt_target as _;
use smart_leds::{
    brightness, gamma,
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite
};

#[embassy_executor::task]
async fn blinky_task(led_pin: GpioPin<8>, rmt: Rmt<'static, esp_hal::Blocking>) {
    info!("Blinky task started");

    let rmt_buffer = smartLedBuffer!(1);
    let mut led = 
        SmartLedsAdapter::new(rmt.channel0, led_pin, rmt_buffer);

    let delay = Duration::from_millis(20);

    let mut color = Hsv {
        hue: 0,
        sat: 255,
        val: 255,
    };
    let mut data;

    info!("Led initialized... -> Starting rainbow from task :)");

    loop {
        for hue in 0..=255 {
            color.hue = hue;

            data = [hsv2rgb(color)];

            led.write(brightness(gamma(data.iter().cloned()), 5)).unwrap();

            Timer::after(delay).await;
        }
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default();
    let peripherals = esp_hal::init(config);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    let led_pin = peripherals.GPIO8;
    let rmt = Rmt::new(peripherals.RMT, Rate::from_mhz(80)).unwrap();

    spawner.spawn(blinky_task(led_pin, rmt)).unwrap();
    info!("Blinky task spawned!");

    loop {
        info!("Main Task running...");
        Timer::after(Duration::from_secs(5)).await;
    }
}
