#![no_std]
#![no_main]

use defmt::info;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[main]
fn main() -> ! {
    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let _peripherals = esp_hal::init(config);

    let mut relay_power = Output::new(_peripherals.GPIO23, Level::Low, OutputConfig::default());
    let mut relay_1 = Output::new(_peripherals.GPIO18, Level::Low, OutputConfig::default());

    let delay = Delay::new();

    relay_power.set_high();
    delay.delay_millis(500);
    relay_1.set_high();

    loop {
        info!("Hello world!");
        relay_1.toggle();
        delay.delay_millis(2500);
    }

    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.0.0-beta.0/examples/src/bin
}
