#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    main,
};
use esp_println::println;

#[main]
fn main() -> ! {

    println!("Hello world!");

    // Initialize the Delay peripheral
    let delay = Delay::new();

    loop {
        println!("Hello World! loop");
        delay.delay_millis(1500);
    }
}
