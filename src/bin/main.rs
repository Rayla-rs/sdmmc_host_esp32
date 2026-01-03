#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

// possible problems
// clock (its gotta be)
// transaction
// reset
// init
// pin config

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::gpio::{Input, InputConfig, OutputConfig};
use esp_hal::interrupt::software::SoftwareInterrupt;
use esp_hal::interrupt::{bind_interrupt, InterruptHandler, Priority};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{clock::CpuClock, gpio::Output};
use esp_hal_embassy::InterruptExecutor;
use log::{debug, info, trace};
use sdio_host::common_cmd::R1;
use sdmmc_host_esp32::{configure_pins, pullup_en_internal, Slot, Width};

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("[PANIC] info={}", info);
    loop {}
}

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.4.0

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz);
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
    configure_pins(true);

    // spawner.must_spawn(sdmmc_host_esp32::intr_poller());

    // let mut d1 = Input::new(peripherals.GPIO2, InputConfig::default());
    let (rx_buf, rx_descs, tx_buf, tx_descs) = esp_hal::dma_buffers!(32000);
    let mut driver = sdmmc_host_esp32::sdmmc_sd::SdmmcCard::new(
        peripherals.SDHOST,
        DmaRxBuf::new(rx_descs, rx_buf).unwrap(),
        DmaTxBuf::new(tx_descs, tx_buf).unwrap(),
    )
    .await;

    driver.cmd_go_idle_state().await.unwrap();

    loop {
        let mut out_rca = 0;
        // let result = driver.cmd_send_op_cond(0x00ff8000, &mut ocrp).await;
        driver.cmd_send_op_cond(0, &mut 0).await;
        // driver.cmd_go_idle_state().await.unwrap();

        Timer::after(Duration::from_secs(1)).await;
    }
}
