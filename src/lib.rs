#![no_std]

mod cmd;
mod common;
mod hw_cmd;
mod sdmmc;
pub mod sdmmc_sd;

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, semaphore::FairSemaphore,
};
use esp_hal::peripherals::IO_MUX;

use crate::inter::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArg,
    Timeout,
    NotFound,
    InvalidCRC,
    InvalidResponce,
    InvalidSize,
    Fail,
    NotSupported,
    InvalidState,
}

//configure pins

const TAG: &'static str = "[SDMMC_HOST_ESP32]";
#[macro_export]
macro_rules! bit {
    ($offset: expr) => {
        1 << $offset
    };
}

pub const SDMMC_FUNC: u8 = 3;
pub const DRIVE_STRENGTH: u8 = 3;

#[macro_export]
macro_rules! configure_pin_iomux {
    ($($pin:ident), *) => {
        let io_mux = unsafe { IO_MUX::steal() };

        $(
            io_mux.register_block().$pin().write(|w| {
                w.fun_wpd().clear_bit();
                w.fun_ie().set_bit();
                unsafe {
                    w.fun_drv().bits(crate::DRIVE_STRENGTH);
                    w.mcu_sel().bits(crate::SDMMC_FUNC)
                }
            });
        )*
    };
}

static EVENT_QUEUE: Channel<CriticalSectionRawMutex, Event, 32> = Channel::new();

static INTR_EVENT: FairSemaphore<CriticalSectionRawMutex, 1> = FairSemaphore::new(0);

const APB_CLK_FREQ: u32 = 80 * 1000000;
// const APB_CLK_FREQ: u32 = 80 * 10000;

mod inter {
    use embassy_sync::semaphore::Semaphore;
    use esp_hal::peripherals::SDHOST;
    use esp_hal::{handler, interrupt::InterruptHandler, system::Cpu};
    use log::{info, trace};

    use crate::{EVENT_QUEUE, TAG};

    // extern crate instability;
    // #[instability::unstable]
    pub fn set_interrupt_handler(interrupt_handler: InterruptHandler) {
        let interrupt = esp32::Interrupt::SDIO_HOST;
        for core in [Cpu::AppCpu, Cpu::ProCpu] {
            esp_hal::interrupt::disable(core, interrupt);
        }
        unsafe { esp_hal::interrupt::bind_interrupt(interrupt, interrupt_handler.handler()) };
        esp_hal::interrupt::enable(interrupt, handler.priority()).unwrap();
    }

    pub unsafe fn bind() {
        set_interrupt_handler(handler);
        esp_hal::interrupt::enable(
            esp32::Interrupt::SDIO_HOST,
            esp_hal::interrupt::Priority::Priority1,
        )
        .unwrap();
        info!("{} bind sdio host intr", TAG);
    }

    #[derive(Debug, Clone, Copy)]
    pub struct Event {
        pub sdmmc_status: u32,
        pub dma_status: u32,
    }

    #[handler(priority = esp_hal::interrupt::Priority::Priority2)]
    pub fn handler() {
        trace!("[SDHOST_INTR] handle");
        let sdmmc = unsafe { SDHOST::steal() };

        info!(
            "[SDHOST_INTR] rst_n reg value={}",
            sdmmc.register_block().rst_n().read().card_reset().bits()
        );

        let pending = sdmmc.register_block().mintsts().read().bits() & 0xFFFF;
        sdmmc
            .register_block()
            .rintsts()
            .write(|w| unsafe { w.bits(pending) });

        let dma_pending = sdmmc.register_block().idsts().read().bits();
        sdmmc
            .register_block()
            .idsts()
            .write(|w| unsafe { w.bits(dma_pending) }); // i don't know why this is here but it is in the c lib

        let event = Event {
            sdmmc_status: pending,
            dma_status: dma_pending,
        };

        info!("[SDHOST_INTR] event {event:?}");

        if pending != 0 || dma_pending != 0 {
            EVENT_QUEUE.try_send(event).unwrap(); // send event
        }

        let sdio_pending = sdmmc
            .register_block()
            .mintsts()
            .read()
            .sdio_interrupt_msk()
            .bits();
        if sdio_pending != 0 {
            sdmmc.register_block().intmask().modify(|r, w| unsafe {
                w.sdio_int_mask()
                    .bits(r.sdio_int_mask().bits() & !sdio_pending)
            });
            super::INTR_EVENT.release(1); // Sephamore release one
        }

        // if task woken yield
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Slot {
    Slot0,
    Slot1,
}

impl Slot {
    #[inline]
    pub fn num(self) -> u8 {
        match self {
            Slot::Slot0 => 0,
            Slot::Slot1 => 1,
        }
    }

    #[inline]
    pub fn bit(self) -> u8 {
        match self {
            Slot::Slot0 => bit!(0),
            Slot::Slot1 => bit!(1),
        }
    }
}

struct SlotInfo {
    width: u8,
    card_detect: u32,
    write_protect: u32,
    card_int: u32,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Width {
    Bit1,
    Bit4,
    Bit8,
}

const SDMMC_SLOT_INFO: [SlotInfo; 2] = [
    SlotInfo {
        width: 8,
        card_detect: 97,
        write_protect: 99,
        card_int: 101,
    },
    SlotInfo {
        width: 4,
        card_detect: 98,
        write_protect: 100,
        card_int: 102,
    },
];

// fn gpio_matrix_test() {
//     let gpio = unsafe { GPIO::steal() };

//     let clk = Output::new(
//         unsafe { esp_hal::peripherals::GPIO2::steal() },
//         esp_hal::gpio::Level::Low,
//         OutputConfig::default()
//             .with_pull(Pull::Up)
//             // .with_drive_mode(esp_hal::gpio::DriveMode::PushPull)
//             .with_drive_strength(esp_hal::gpio::DriveStrength::_20mA),
//     );
//     let mut cmd = Flex::new(unsafe { esp_hal::peripherals::GPIO2::steal() });
//     cmd.set_input_enable(true);
//     cmd.set_output_enable(true);
//     let mut d0 = Flex::new(unsafe { esp_hal::peripherals::GPIO2::steal() });
//     d0.set_input_enable(true);
//     d0.set_output_enable(true);

//     // configure_pin_gpio_matrix(14, , false, true);
// }

mod gpio {
    use esp_hal::peripherals::{GPIO, IO_MUX};

    pub fn configure_pin_gpio_matrix(gpio_num: u8, sig: usize, input: bool, output: bool) {
        let gpio = unsafe { GPIO::steal() };

        gpio_reset_pin(gpio_num as usize);
        gpio_set_direction(gpio_num as usize, input, output);
        gpio_pulldown_dis(gpio_num as usize);

        // connect in sig
        if input {
            gpio.register_block()
                .func_in_sel_cfg(sig)
                .write(|w| unsafe { w.in_sel().bits(gpio_num) });
        }

        // connect out sig
        if output {
            gpio.register_block()
                .func_out_sel_cfg(sig)
                .write(|w| unsafe { w.out_sel().bits(gpio_num as u16) });
        }
    }

    fn gpio_pulldown_dis(gpio_num: usize) {
        todo!()
    }

    pub fn gpio_set_direction(gpio_num: usize, input: bool, output: bool) {
        let gpio = unsafe { GPIO::steal() };
        // let io_mux = unsafe { IO_MUX::steal() };

        if input {
            // done by io_mux
        }
        if output {
            if gpio_num < 32 {
                gpio.register_block()
                    .enable_w1ts()
                    .write(|w| unsafe { w.enable_data_w1ts().bits(bit!(gpio_num)) });
            } else {
                gpio.register_block()
                    .enable1_w1ts()
                    .write(|w| unsafe { w.enable1_data_w1ts().bits(bit!(gpio_num - 32)) });
            }
        } else {
            if gpio_num < 32 {
                gpio.register_block()
                    .enable_w1tc()
                    .write(|w| unsafe { w.enable_data_w1tc().bits(bit!(gpio_num)) });
            } else {
                gpio.register_block()
                    .enable1_w1tc()
                    .write(|w| unsafe { w.enable1_data_w1tc().bits(bit!(gpio_num - 32)) });
            }
        }
    }

    fn gpio_reset_pin(gpio_num: usize) {
        gpio_intr_disable(gpio_num);
    }

    fn gpio_intr_disable(gpio_num: usize) {
        let gpio = unsafe { GPIO::steal() };

        gpio.register_block()
            .pin(gpio_num)
            .write(|w| unsafe { w.int_ena().bits(0) });

        if gpio_num < 32 {
            gpio.register_block()
                .status_w1tc()
                .write(|w| unsafe { w.status_int_w1tc().bits(bit!(gpio_num)) });
        } else {
            gpio.register_block()
                .status1_w1tc()
                .write(|w| unsafe { w.status1_int_w1tc().bits(bit!(gpio_num - 32)) });
        }
    }
}

pub fn pullup_en_internal(slot: Slot, width: Width) -> Result<(), Error> {
    let io_mux = unsafe { IO_MUX::steal() };

    macro_rules! pullup_en {
        ( $( $pin: ident), *) => {
            $(
                io_mux.register_block().$pin().write(|w| w.fun_wpu().set_bit());
            )*
        };
    }

    match (slot, width) {
        (Slot::Slot0, Width::Bit1) => {
            pullup_en!(gpio11, gpio6);
        }
        (Slot::Slot0, Width::Bit4) => {
            pullup_en!(gpio11, gpio6, gpio7, gpio8, gpio9, gpio10);
        }
        (Slot::Slot0, Width::Bit8) => {
            pullup_en!(gpio11, gpio6, gpio7, gpio8, gpio9, gpio10, gpio16, gpio17, gpio5, gpio18);
        }
        (Slot::Slot1, Width::Bit1) => {
            pullup_en!(gpio15, gpio14);
        }
        (Slot::Slot1, Width::Bit4) => {
            pullup_en!(gpio15, gpio14, gpio2, gpio4, gpio12, gpio13);
        }
        _ => {
            Err(Error::InvalidArg)?;
        }
    }
    Ok(())
}

pub fn configure_pins(enable_pullups: bool) {
    let io_mux = unsafe { IO_MUX::steal() };
    // CLK (GPIO14) - host-driven output but keep input enabled (esp-idf does gpio_input_enable())
    io_mux.register_block().gpio14().write(|w| {
        w.mcu_wpd().clear_bit(); // disable pulldown
        if enable_pullups {
            w.mcu_wpu().set_bit(); // optional host pullup
        } else {
            w.mcu_wpu().clear_bit();
        }
        w.mcu_ie().set_bit(); // enable input (driver enables input even for CLK)
        w.mcu_oe().set_bit(); // allow peripheral to drive the pad
                              // unsafe { w.fun_drv().bits(DRIVE_STRENGTH) }; // optional: set function drive strength
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) } // select SDMMC IOMUX function
    });

    // CMD (GPIO15) - bidirectional
    io_mux.register_block().gpio15().write(|w| {
        w.mcu_wpd().clear_bit();
        if enable_pullups {
            w.mcu_wpu().set_bit();
        } else {
            w.mcu_wpu().clear_bit();
        }
        w.mcu_ie().set_bit(); // enable input
        w.mcu_oe().set_bit(); // allow peripheral to drive when needed
                              // unsafe { w.fun_drv().bits(DRIVE_STRENGTH) };
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) }
    });

    // D0 (GPIO2) - bidirectional
    io_mux.register_block().gpio2().write(|w| {
        w.mcu_wpd().clear_bit();
        if enable_pullups {
            w.mcu_wpu().set_bit();
        } else {
            w.mcu_wpu().clear_bit();
        }
        w.mcu_ie().set_bit();
        w.mcu_oe().set_bit();
        // unsafe { w.fun_drv().bits(DRIVE_STRENGTH) };
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) }
    });
}
pub fn configure_pins2(enable_pullups: bool) {
    let io_mux = unsafe { IO_MUX::steal() };
    // CLK (GPIO14) - host-driven output but keep input enabled (esp-idf does gpio_input_enable())
    io_mux.register_block().gpio14().write(|w| {
        w.mcu_ie().set_bit();
        w.fun_ie().set_bit();
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) } // select SDMMC IOMUX function
    });

    // CMD (GPIO15) - bidirectional
    io_mux.register_block().gpio15().write(|w| {
        w.mcu_ie().set_bit();
        w.fun_ie().set_bit();
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) } // select SDMMC IOMUX function
    });

    // D0 (GPIO2) - bidirectional
    io_mux.register_block().gpio2().write(|w| {
        w.mcu_ie().set_bit();
        w.fun_ie().set_bit();
        unsafe { w.mcu_sel().bits(SDMMC_FUNC) } // select SDMMC IOMUX function
    });

    {
        let rtc = unsafe { esp_hal::peripherals::RTC_IO::steal() };
        let block = rtc.register_block();
        block.touch_pad2().write(|w| w.rde().clear_bit());
        block
            .touch_pad3()
            .write(|w| w.rde().clear_bit().rue().set_bit());
        block
            .touch_pad6()
            .write(|w| w.rde().clear_bit().rue().set_bit());
    }
}
