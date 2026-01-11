use core::{future::Future, ops::Not, task::Poll};

use embassy_futures::yield_now;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    semaphore::{FairSemaphore, Semaphore},
};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    dma::DmaDescriptor,
    peripherals::{self, IO_MUX, SDHOST},
};
use esp_hal::{
    gpio::{Output, OutputConfig},
    peripherals::DPORT,
};
use log::{debug, info, warn};

mod ll;

const TAG: &'static str = "[SDMMC]";

use crate::{
    bit, configure_pin_iomux,
    hw_cmd::SdmmcHwCmd,
    inter::{self, Event},
    pullup_en_internal,
    sdmmc::ll::SDMMC_LL_EVENT_DEFAULT,
    Error, Slot, Width, APB_CLK_FREQ, EVENT_QUEUE, INTR_EVENT,
};

const CLK_SRC_HZ: u32 = 160 * 1000000;

#[non_exhaustive]
enum ClockSource {
    PLL160M,
}

#[derive(Default)]
struct SlotCtx {
    slot_freq_khz: u32,
    slot_host_div: u8,
    use_gpio_matrix: bool,
}

pub struct Sdmmc {
    pub host: SDHOST<'static>,
    pub slot_ctx: [SlotCtx; 2],
    active_slot: Option<Slot>,
}

impl Sdmmc {
    pub fn new(host: SDHOST<'static>) -> Self {
        Self {
            host,
            slot_ctx: [Default::default(), Default::default()],
            active_slot: None,
        }
    }
}

impl Sdmmc {
    fn module_reset(&self) {
        self.ll_reset_controller();
        self.ll_reset_dma();
        self.ll_reset_fifo();
    }

    fn is_module_reset_done(&self) -> bool {
        self.ll_is_controller_reset_done()
            && self.ll_is_dma_reset_done()
            && self.ll_is_fifo_reset_done()
    }

    pub async fn reset(&mut self) -> Result<(), Error> {
        self.module_reset();

        const RESET_TIMEOUT_MS: u64 = 5000;
        let mut yield_delay_ms = Duration::from_millis(100);
        let t0 = Instant::now();
        let mut t1;

        while !self.is_module_reset_done() {
            t1 = Instant::now();
            if t1 - t0 > Duration::from_millis(RESET_TIMEOUT_MS) {
                Err(Error::Timeout)?;
            } else if t1 - t0 > yield_delay_ms {
                yield_delay_ms = Duration::from_millis(yield_delay_ms.as_millis() * 2);
                yield_now().await;
            }
        }

        Ok(())
    }

    pub async fn set_clk_div(&mut self, div: u8) {
        // esp_clk_tree_enable_src not needed
        self.ll_set_clk_div(div);
        self.ll_select_clk_src(); // for compatibility
        self.ll_init_phase_delay();

        // Wait for the clock to propagate
        embassy_time::Timer::after_micros(10).await
    }

    pub async fn clk_update_cmd(&mut self, slot: Slot, is_cmd11: bool) -> Result<(), Error> {
        self.start_cmd(
            slot,
            SdmmcHwCmd::default()
                .with_card_num(slot.num())
                .with_update_clk_reg(true)
                .with_wait_complete(true)
                .with_volt_switch(is_cmd11),
            0,
        )
        .await
        .inspect_err(|err| warn!("{TAG} start_cmd returned {err:?}"))
    }

    pub fn get_clk_divs(&self, freq_khz: u32) -> (u8, u8) {
        // self.host.clk_edge_sel().read().ccllkin_edge_h().bits()
        let clk_src_freq_hz = CLK_SRC_HZ;

        const HIGHSPEED: u32 = 40000;
        const DEFAULT: u32 = 20000;
        const PROBING: u32 = 400;

        if freq_khz >= HIGHSPEED {
            (4, 0)
        } else if freq_khz == DEFAULT {
            (8, 0)
        } else if freq_khz == PROBING {
            (10, 20)
        } else {
            let mut host_div = (clk_src_freq_hz) / (freq_khz * 1000);
            let mut card_div = 0;
            if host_div > 15 {
                host_div = 2;
                card_div = (clk_src_freq_hz / 2) / (2 * freq_khz * 1000);
                if (clk_src_freq_hz / 2) % (2 * freq_khz * 1000) > 0 {
                    card_div += 1;
                }
            } else if clk_src_freq_hz % (freq_khz * 1000) > 0 {
                host_div += 1;
            }

            (host_div as u8, card_div as u8)
        }
    }

    pub fn calc_freq(&self, host_div: u8, card_div: u8) -> u32 {
        let clk_src_freq_hz = CLK_SRC_HZ;
        let (host_div, card_div) = (host_div as u32, card_div as u32);
        clk_src_freq_hz / host_div / if card_div == 0 { 1 } else { card_div * 2 } / 1000
    }

    pub fn set_data_timeout(&self, freq_khz: u32) {
        const DATA_TIMEOUT_MS: u32 = 100;
        let data_timeout_cycles = DATA_TIMEOUT_MS * freq_khz;
        self.ll_set_data_timeout(data_timeout_cycles);
    }

    pub async fn set_card_clk(&mut self, slot: Slot, freq_khz: u32) -> Result<(), Error> {
        // Disable clock first
        self.ll_enable_card_clk(slot, false);

        self.clk_update_cmd(slot, false).await.inspect_err(|err| {
            warn!("{TAG} disableing clk failed");
            warn!("{TAG} clk_update_cmd returned {err:?}")
        })?;

        let (host_div, card_div) = self.get_clk_divs(freq_khz);

        let real_freq = self.calc_freq(host_div, card_div);
        info!("{TAG} slot={slot:?} clk_src=default host_div={host_div} card_div={card_div} freq={real_freq}khz (max {freq_khz}khz)");

        // Program card clock settings, send them to the CIU
        self.ll_set_card_clk_div(slot, card_div);
        self.set_clk_div(host_div);
        self.clk_update_cmd(slot, false).await.inspect_err(|err| {
            warn!("{TAG} setting clk div failed");
            warn!("{TAG} clk_update_cmd returned {err:?}")
        })?;

        // Re-enable clocks
        self.ll_enable_card_clk(slot, true);
        self.ll_enable_card_clk_low_power(slot, true);
        self.clk_update_cmd(slot, false).await.inspect_err(|err| {
            warn!("{TAG} re-enabling clk div failed");
            warn!("{TAG} clk_update_cmd returned {err:?}")
        })?;

        self.set_data_timeout(freq_khz);
        self.ll_set_responce_timeout(255);
        self.slot_ctx[slot.num() as usize].slot_freq_khz = freq_khz;
        self.slot_ctx[slot.num() as usize].slot_host_div = host_div;
        Ok(())
    }

    fn set_input_delay(_slot: Slot, _delay_phase: u32) -> Result<(), Error> {
        warn!("{TAG} esp32 doesn't support input phase delay, fallback to 0 delay");
        Err(Error::NotSupported)
    }

    pub async fn start_cmd(
        &mut self,
        slot: Slot,
        mut cmd: SdmmcHwCmd,
        arg: u32,
    ) -> Result<(), Error> {
        // Change host settings to apropriate slot
        if self.active_slot.is_none_or(|active| active != slot) {
            if self.slot_initialized(slot) {
                self.active_slot = Some(slot);
                self.change_to_slot(slot).await;
            } else {
                debug!("{TAG} Slot {slot:?} is not initialized yet, skipped change_to_slot")
            }
        }

        if self.ll_is_card_detected(slot) && !cmd.update_clk_reg() {
            Err(Error::NotFound)?;
        }

        if cmd.data_expected() && cmd.rw() && self.ll_is_card_write_protected(slot) {
            Err(Error::NotFound)?;
        }

        cmd = cmd.with_use_hold_reg(true);

        let mut yield_return_thresh = esp_hal::time::Duration::from_millis(100);
        let t0 = esp_hal::time::Instant::now();
        let mut t1;
        const TIMEOUT_US: esp_hal::time::Duration = esp_hal::time::Duration::from_millis(1000);

        if !(cmd.volt_switch() && cmd.update_clk_reg()) {
            while !self.cmd_taken() {
                t1 = esp_hal::time::Instant::now();
                if t1 - t0 > TIMEOUT_US {
                    info!("{TAG} timeout while awaiting cmd_taken");
                    Err(Error::Timeout)?;
                }
                if t1 - t0 > yield_return_thresh {
                    yield_return_thresh =
                        esp_hal::time::Duration::from_millis(yield_return_thresh.as_millis() * 2);
                    yield_now().await;
                }
            }
        }
        self.ll_set_cmd_arg(arg);
        cmd = cmd.with_card_num(slot.num()).with_start_command(true);
        self.ll_set_cmd(cmd);

        while !self.ll_is_command_taken() {
            let t1 = esp_hal::time::Instant::now();
            if t1 - t0 > TIMEOUT_US {
                info!("{TAG} timeout awaiting cmd_taken (end of start_cmd)");
                Err(Error::Timeout)?;
            }
            if t1 - t0 > yield_return_thresh {
                yield_return_thresh =
                    esp_hal::time::Duration::from_millis(yield_return_thresh.as_millis() * 2);
                yield_now().await;
            }
        }
        Ok(())
    }

    fn intmask_clear_disable(&self) {
        self.ll_clear_interrupt(0xffffffff);
        self.ll_enable_interrupt(0xffffffff, false);
        self.ll_enable_global_interrupt(false);
    }

    fn intmask_set_enable(&self) {
        self.ll_enable_interrupt(0xffffffff, false);
        self.ll_enable_interrupt(SDMMC_LL_EVENT_DEFAULT, true);
        self.ll_enable_global_interrupt(true);
    }

    pub async fn init(&mut self) -> Result<(), Error> {
        self.ll_enable_bus_clk(true);
        self.ll_reset_register();

        self.set_clk_div(2).await;

        self.reset()
            .await
            .inspect_err(|err| warn!("{TAG} init: reset returned {err:?}"))?;

        debug!(
            "{TAG} peripheral_version={} hardware_config={}",
            self.ll_get_version_id(),
            self.ll_get_hw_config_info(),
        );

        // Clear interrupt status
        self.intmask_clear_disable();

        // Alloc Event Queue
        EVENT_QUEUE.clear();

        // Reset Semaphore
        INTR_EVENT.set(0);

        // Attack interrupt handler
        unsafe { inter::bind() };

        // Enable interrupts
        self.intmask_set_enable();

        // Disable generation of busy clear inter
        self.ll_enable_busy_clear_interrupt(false);

        // Enable DMA
        self.ll_init_dma();

        warn!("{TAG} transaction handler initializeation ignored");

        Ok(())
    }
    // NOTE Above is done

    pub fn configure_pin_iomux(pin: u8) {
        //
    }

    pub async fn init_slot(
        &mut self,
        slot: Slot,
        width: Width,
        freq_khz: &mut u32,
    ) -> Result<(), Error> {
        pullup_en_internal(slot, width)?;
        configure_pin_iomux!(gpio15, gpio14, gpio2);

        let pwr = Output::new(
            unsafe { esp_hal::peripherals::GPIO13::steal() },
            esp_hal::gpio::Level::High,
            OutputConfig::default().with_pull(esp_hal::gpio::Pull::None),
        );

        // Card Int -> NC

        // Card Detect
        // let a = unsafe {
        //     esp_hal::peripherals::GPIO34::steal()
        //         .split()
        //         .0
        //         .with_input_inverter(false)
        // };

        self.set_card_clk(slot, *freq_khz).await?;
        self.set_bus_width(slot, width)?;

        // Set up Write Protect Input

        Ok(())
    }

    pub fn set_bus_width(&self, slot: Slot, width: Width) -> Result<(), Error> {
        if slot == Slot::Slot1 && width == Width::Bit8 {
            Err(Error::InvalidArg)?;
        }
        let mask = slot as u8;
        match width {
            Width::Bit1 => {
                self.host.register_block().ctype().modify(|r, w| unsafe {
                    w.card_width4().bits(r.card_width4().bits() & !mask);
                    w.card_width8().bits(r.card_width8().bits() & !mask)
                });
            }
            Width::Bit4 => {
                todo!()
            }
            Width::Bit8 => {
                todo!()
            }
        }
        log::trace!("{} slot={:?} width={:?}", TAG, slot, width);

        Ok(())
    }
    pub fn set_clk_always_on(&mut self, slot: Slot, en: bool) {
        // mut because of safety
        self.host.register_block().clkena().modify(|r, w| unsafe {
            w.lp_enable().bits(if en {
                r.lp_enable().bits() | slot as u8
            } else {
                r.lp_enable().bits() & !(slot as u8)
            })
        });
    }

    pub async fn wait_for_event(&self) -> Event {
        EVENT_QUEUE.receive().await
    }

    // DMA

    pub fn dma_init(&self) {
        let block = self.host.register_block();

        block
            .ctrl()
            .modify(|r, w| unsafe { w.bits(r.bits() | 1 << 5) }); // enable dma
        block.bmod().write(|w| unsafe { w.bits(0) });
        block.bmod().write(|w| w.swr().set_bit());
        block.idinten().write(|w| {
            w.ni().set_bit();
            w.ri().set_bit();
            w.ti().set_bit()
        });
    }

    pub fn dma_stop(&self) {
        let block = self.host.register_block();

        block.ctrl().write(|w| w.dma_reset().set_bit());
        block
            .ctrl()
            .modify(|r, w| unsafe { w.bits(r.bits() & !(1 << 25)) }); // disable dma internal

        block.bmod().write(|w| {
            w.de().clear_bit(); // Double check bit
            w.fb().clear_bit()
        });
    }

    pub fn dma_prepare(&self, desc: *mut DmaDescriptor, block_size: u16, data_size: u32) {
        let block = self.host.register_block();

        block
            .bytcnt()
            .write(|w| unsafe { w.byte_count().bits(data_size) });
        block
            .blksiz()
            .write(|w| unsafe { w.block_size().bits(block_size) });
        block
            .dbaddr()
            .write(|w| unsafe { w.dbaddr().bits(desc.addr() as u32) });

        // Other
        block.bmod().write(|w| {
            w.de().set_bit(); // Double check bit
            w.fb().set_bit()
        });
        self.dma_resume();
    }

    pub fn dma_resume(&self) {
        self.host
            .register_block()
            .pldmnd()
            .write(|w| unsafe { w.pd().bits(1) });
    }

    pub fn cmd_taken(&self) -> bool {
        self.host
            .register_block()
            .cmd()
            .read()
            .start_cmd()
            .bit_is_clear()
    }

    pub async fn enable_clk_cmd11(&mut self, slot: Slot, en: bool) -> Result<(), Error> {
        self.enable_card_clock(slot, en);
        self.clk_update_cmd(slot, true).await?;
        self.enable_1v8_mode(slot, en);
        Ok(())
    }

    pub fn enable_card_clock(&self, slot: Slot, en: bool) {
        self.host.register_block().clkena().modify(|r, w| unsafe {
            w.cclk_enable().bits(if en {
                r.cclk_enable().bits() | slot as u8
            } else {
                r.cclk_enable().bits() & !(slot as u8)
            })
        });
    }

    pub fn enable_1v8_mode(&self, slot: Slot, en: bool) {
        // for compatibility
    }

    pub fn set_card_width(&self, slot: Slot, width: Width) {
        self.host.register_block().ctype().modify(|r, w| unsafe {
            match width {
                Width::Bit1 => {
                    w.card_width8().bits(r.card_width8().bits() & !(slot as u8));
                    w.card_width4().bits(r.card_width4().bits() & !(slot as u8))
                }
                Width::Bit4 => {
                    w.card_width8().bits(r.card_width8().bits() & !(slot as u8));
                    w.card_width4().bits(r.card_width4().bits() | (slot as u8))
                }
                Width::Bit8 => w.card_width8().bits(r.card_width8().bits() | (slot as u8)),
            }
        });
    }

    pub fn enable_dma(&self, en: bool) {
        let block = self.host.register_block();

        let mask = (1 << 5) | (1 << 25);
        block.ctrl().modify(|r, w| unsafe {
            w.bits(if en {
                r.bits() | mask
            } else {
                r.bits() & !mask
            })
        }); // enable dma and dma internal
        block.bmod().write(|w| {
            w.fb().variant(en);
            w.de().variant(en)
        });
    }

    fn slot_initialized(&self, slot: Slot) -> bool {
        self.slot_ctx[slot.num() as usize].slot_host_div != 0
    }

    async fn change_to_slot(&self, slot: Slot) {
        self.ll_set_clk_div(self.slot_ctx[slot.num() as usize].slot_host_div);
        self.set_data_timeout(self.slot_ctx[slot.num() as usize].slot_freq_khz);

        // Wait for the clock to propagate
        embassy_time::Timer::after_micros(10).await
    }
}
