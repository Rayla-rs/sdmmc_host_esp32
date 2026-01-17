#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::gpio::{Input, InputConfig, Output, OutputConfig};
use esp_hal::timer::timg::TimerGroup;
use log::{info, warn};
use sdmmc_host_esp32::{configure_pins2, pullup_en_internal, Slot, Width};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("[PANIC] info={}", info);
    loop {}
}

const TAG: &'static str = "[MAIN]";

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.4.0

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::_80MHz);
    let peripherals = esp_hal::init(config);

    Input::new(peripherals.GPIO34, InputConfig::default());
    unsafe {
        esp_hal::peripherals::GPIO::steal()
            .register_block()
            .func_in_sel_cfg(98)
            .write(|w| w.in_sel().bits(34))
    };

    let timer0 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timer0.timer0);

    info!("Embassy initialized!");

    Output::new(
        peripherals.GPIO13,
        esp_hal::gpio::Level::Low,
        OutputConfig::default().with_pull(esp_hal::gpio::Pull::None),
    );
    pullup_en_internal(Slot::Slot1, Width::Bit1).unwrap();
    configure_pins2(true);

    // spawner.must_spawn(sdmmc_host_esp32::intr_poller());

    // let mut d1 = Input::new(peripherals.GPIO2, InputConfig::default());
    let (rx_buf, rx_descs, tx_buf, tx_descs) = esp_hal::dma_buffers!(32000);
    let mut driver = sdmmc_host_esp32::sdmmc_sd::SdmmcCard::new(
        peripherals.SDHOST,
        DmaRxBuf::new(rx_descs, rx_buf).unwrap(),
        DmaTxBuf::new(tx_descs, tx_buf).unwrap(),
    )
    .await;

    // try init
    while let Err(err) = driver.init().await {
        warn!("{TAG} driver init failed with err={err:?}, retry...");
        Timer::after(Duration::from_secs(1)).await
    }
    info!("{TAG} Driver initialized!");
}
