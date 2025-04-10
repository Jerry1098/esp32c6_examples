#![no_std]
#![no_main]

use defmt::info;
use esp_hal::delay::Delay;
use esp_hal::main;
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal_smartled::{smartLedBuffer, SmartLedsAdapter};
use panic_rtt_target as _;
use smart_leds::{
    brightness, gamma,
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite,
};

#[main]
fn main() -> ! {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    info!("Hello World!");

    let config = esp_hal::Config::default(); // .with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // for esp32c6
    let led_pin = peripherals.GPIO8;
    let freq = Rate::from_mhz(80);

    let rmt = Rmt::new(peripherals.RMT, freq).unwrap();

    // Using one RMT channel to instantiate a 'SmartLedsAdapter' which can
    // be used directly with all `smart_led` implementations
    let rmt_buffer = smartLedBuffer!(1);
    let mut led = SmartLedsAdapter::new(rmt.channel0, led_pin, rmt_buffer);

    let delay = Delay::new();

    let mut color = Hsv {
        hue: 0,
        sat: 255,
        val: 255,
    };
    let mut data;

    info!("Led initialized... -> Starting rainbow :)");

    loop {
        for hue in 0..=255 {
            color.hue = hue;

            // Convert from HSV to RGB
            data = [hsv2rgb(color)];

            // When sending to the LED, we do a gamma correction first (see smart_leds documentation)
            // and then limit the brightness to 10 out of 255 so that the output is not to bright.
            led.write(brightness(gamma(data.iter().cloned()), 5))
                .unwrap();

            delay.delay_millis(20);
        }
        info!("Rainbow finished, restarting");
    }
}
