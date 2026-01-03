use core::{future::Future, ops::Not, task::Poll};

use embassy_futures::yield_now;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    semaphore::{FairSemaphore, Semaphore},
};
use embassy_time::{Duration, Instant};
use esp_hal::{
    dma::DmaDescriptor,
    peripherals::{self, IO_MUX, SDHOST},
};
use esp_hal::{
    gpio::{Output, OutputConfig},
    peripherals::DPORT,
};
use log::{info, warn};

const TAG: &'static str = "[SDMMC]";

use crate::{
    bit, configure_pin_iomux,
    hw_cmd::SdmmcHwCmd,
    inter::{self, Event},
    pullup_en_internal, Error, Slot, Width, APB_CLK_FREQ, EVENT_QUEUE, INTR_EVENT,
};

pub struct Sdmmc {
    pub host: SDHOST<'static>,
}

impl Sdmmc {
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

    pub async fn reset(&mut self) -> Result<(), Error> {
        self.host.register_block().ctrl().write(|w| {
            w.controller_reset().set_bit();
            w.dma_reset().set_bit();
            w.fifo_reset().set_bit()
        });

        const RESET_TIMEOUT_MS: u64 = 5000;
        let mut yield_delay_ms = Duration::from_millis(100);
        let t0 = Instant::now();
        let mut t1;

        let ctrl = self.host.register_block().ctrl();
        while !(ctrl.read().controller_reset().bit_is_clear()
            && ctrl.read().dma_reset().bit_is_clear()
            && ctrl.read().fifo_reset().bit_is_clear())
        {
            t1 = Instant::now();
            if t1 - t0 > Duration::from_millis(RESET_TIMEOUT_MS) {
                warn!(
                    "{TAG} reset timeout: controller_reset={} dma_reset={} fifo_reset={}",
                    ctrl.read().controller_reset().bit(),
                    ctrl.read().dma_reset().bit(),
                    ctrl.read().fifo_reset().bit()
                );
                Err(Error::Timeout)?;
            } else if t1 - t0 > yield_delay_ms {
                yield_delay_ms = Duration::from_millis(yield_delay_ms.as_millis() * 2);
                yield_now().await;
            }
        }

        log::debug!("{} reader_state={:b}", TAG, ctrl.read().bits());
        Ok(())
    }

    pub async fn set_clk_div(&mut self, div: u8) {
        assert!(div > 1 && div <= 16);
        let h = div - 1;
        let l = div / 2 - 1;

        self.host.register_block().clk_edge_sel().write(|w| unsafe {
            w.ccllkin_edge_h().bits(h);
            w.ccllkin_edge_l().bits(l);
            w.ccllkin_edge_n().bits(h);

            w.cclkin_edge_drv_sel().bits(4);
            w.cclkin_edge_sam_sel().bits(4);
            w.cclkin_edge_slf_sel().bits(0)
        });

        embassy_time::Timer::after_micros(10).await
    }

    pub fn get_clock_div(&self) -> u8 {
        self.host
            .register_block()
            .clk_edge_sel()
            .read()
            .ccllkin_edge_h()
            .bits()
            + 1
    }

    pub fn get_card_clock_div(&self, slot: Slot) -> u8 {
        let reader = self.host.register_block().clkdiv().read();
        match slot {
            Slot::Slot0 => reader.clk_divider0().bits(),
            Slot::Slot1 => reader.clk_divider1().bits(),
        }
    }

    pub fn get_clk_divs(&self, freq_khz: u32) -> (u8, u8) {
        // self.host.clk_edge_sel().read().ccllkin_edge_h().bits()
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
            let mut host_div = (2 * APB_CLK_FREQ) / (freq_khz * 1000);
            let mut card_div = 0;
            if host_div > 15 {
                host_div = 2;
                card_div = APB_CLK_FREQ / (2 * freq_khz * 1000);
                if (APB_CLK_FREQ % (2 * freq_khz * 1000)) > 0 {
                    card_div += 1;
                }
            } else if ((2 * APB_CLK_FREQ) % (freq_khz * 1000)) > 0 {
                host_div += 1;
            }

            (host_div as u8, card_div as u8)
        }
    }

    pub async fn clk_update_cmd(&self, slot: Slot, is_cmd11: bool) -> Result<(), Error> {
        self.start_cmd(
            slot,
            SdmmcHwCmd::default()
                .with_card_num(slot as u8)
                .with_update_clk_reg(true)
                .with_wait_complete(true)
                .with_volt_switch(is_cmd11),
            0,
        )
        .await
    }

    pub async fn start_cmd(&self, slot: Slot, mut cmd: SdmmcHwCmd, arg: u32) -> Result<(), Error> {
        if slot as u8
            & self
                .host
                .register_block()
                .cdetect()
                .read()
                .card_detect_n()
                .bits()
            != 0
            && !cmd.update_clk_reg()
        {
            Err(Error::NotFound)?;
        }

        if cmd.data_expected()
            && cmd.rw()
            && slot as u8
                & self
                    .host
                    .register_block()
                    .wrtprt()
                    .read()
                    .write_protect()
                    .bits()
                != 0
        {
            Err(Error::NotFound)?;
        }

        cmd = cmd.with_use_hold_reg(true);
        let mut yield_return_thresh = esp_hal::time::Duration::from_millis(100);
        let t0 = esp_hal::time::Instant::now();

        const TIMEOUT_US: esp_hal::time::Duration = esp_hal::time::Duration::from_millis(1000);

        if !(cmd.volt_switch() && cmd.update_clk_reg()) {
            while !self.cmd_taken() {
                let t1 = esp_hal::time::Instant::now();
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
        self.host
            .register_block()
            .cmdarg()
            .write(|w| unsafe { w.cmdarg().bits(arg) });
        cmd = cmd.with_card_num(slot as u8).with_start_command(true);
        self.host
            .register_block()
            .cmd()
            .write(|w| unsafe { w.bits(cmd.0) });

        while !self.cmd_taken() {
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

    pub async fn init(&mut self) -> Result<(), Error> {
        // reset

        let dport = unsafe { DPORT::steal() };
        let block = dport.register_block();

        // Reset
        block.wifi_rst_en().write(|w| w.sdio_host_rst().set_bit());
        block.wifi_rst_en().write(|w| w.sdio_host_rst().clear_bit());
        block
            .peri_rst_en()
            .modify(|r, w| unsafe { w.peri_rst_en().bits(r.peri_rst_en().bits() | bit!(20)) });
        block
            .peri_clk_en()
            .modify(|r, w| unsafe { w.peri_clk_en().bits(r.peri_clk_en().bits() | bit!(20)) });
        block
            .peri_rst_en()
            .modify(|r, w| unsafe { w.peri_rst_en().bits(r.peri_rst_en().bits() & !bit!(20)) });

        self.host
            .register_block()
            .rst_n()
            .write(|w| unsafe { w.card_reset().bits(0b00) });
        self.host
            .register_block()
            .clkena()
            .write(|w| unsafe { w.cclk_enable().bits(0b11) });
        // self.host.rst_n().write(|w| w.card_reset())

        self.set_clk_div(2).await;

        self.reset().await?;

        log::trace!(
            "{} peripheral_version={} hardware_config={}",
            TAG,
            self.host.register_block().verid().read().versionid().bits(),
            self.host.register_block().hcon().read().bits()
        );

        // Clear interrupt status
        self.host
            .register_block()
            .rintsts()
            .write(|w| unsafe { w.bits(0xffffffff) });
        self.host
            .register_block()
            .intmask()
            .write(|w| unsafe { w.bits(0) });
        self.host
            .register_block()
            .ctrl()
            .write(|w| w.int_enable().clear_bit());

        // Alloc Event Queue
        EVENT_QUEUE.clear();

        // Reset Semaphore
        INTR_EVENT.set(0);

        unsafe { inter::bind() };

        // Enable interrupts
        self.host.register_block().intmask().write(|w| unsafe {
            w.int_mask().bits(
                0xffff
                    | bit!(0)
                    | bit!(2)
                    | bit!(3)
                    | bit!(6)
                    | bit!(7)
                    | bit!(8)
                    | bit!(9)
                    | bit!(10)
                    | bit!(13)
                    | bit!(15)
                    | bit!(1)
                    | bit!(12),
            )
        });
        self.host.register_block().idinten().write(|w| {
            w.ti().set_bit();
            w.ri().set_bit();
            w.fbe().set_bit();
            w.du().set_bit();
            w.ces().set_bit();
            w.ni().set_bit();
            w.ai().set_bit()
        });

        self.host
            .register_block()
            .ctrl()
            .write(|w| w.int_enable().set_bit());

        // Disable generation of busy clear inter
        self.host
            .register_block()
            .cardthrctl()
            .write(|w| w.cardclrinten().clear_bit());

        // Enable DMA
        self.dma_init();

        Ok(())
    }

    pub async fn set_card_clk(&mut self, slot: Slot, freq_khz: &mut u32) -> Result<(), Error> {
        // Disable clock
        self.host
            .register_block()
            .clkena()
            .modify(|r, w| unsafe { w.cclk_enable().bits(r.cclk_enable().bits() & !(slot as u8)) });
        self.clk_update_cmd(slot, false).await?;

        let (host_div, card_div) = self.get_clk_divs(*freq_khz);

        let real_freq = self.calc_freq(host_div, card_div);
        *freq_khz = real_freq; // * 1000;
        warn!("[HERE] {freq_khz}");
        // Program CLKDIV and CLKSRC, send them to the CIU
        match slot {
            Slot::Slot0 => {
                self.host
                    .register_block()
                    .clksrc()
                    .modify(|r, w| unsafe { w.clksrc().bits(r.clksrc().bits() & 0b0011) });
                self.host
                    .register_block()
                    .clkdiv()
                    .write(|w| unsafe { w.clk_divider0().bits(card_div) });
            }
            Slot::Slot1 => {
                self.host
                    .register_block()
                    .clksrc()
                    .modify(|r, w| unsafe { w.clksrc().bits(r.clksrc().bits() & 0b1100 | 0b0010) });
                self.host
                    .register_block()
                    .clkdiv()
                    .write(|w| unsafe { w.clk_divider1().bits(card_div) });
            }
        }

        self.set_clk_div(host_div).await;
        self.clk_update_cmd(slot, false).await?;

        // Re-enable clocks
        self.host.register_block().clkena().modify(|r, w| unsafe {
            w.cclk_enable().bits(r.cclk_enable().bits() | slot as u8);
            w.lp_enable().bits(r.lp_enable().bits() | slot as u8)
        });
        self.clk_update_cmd(slot, false).await?;

        // Set data timeout
        self.host
            .register_block()
            .tmout()
            .write(|w| unsafe { w.data_timeout().bits((100 * *freq_khz).min(0xffffff)) });
        self.host
            .register_block()
            .tmout()
            .write(|w| unsafe { w.response_timeout().bits(255) });

        Ok(())
    }

    pub fn calc_freq(&self, host_div: u8, card_div: u8) -> u32 {
        let (host_div, card_div) = (host_div as u32, card_div as u32);
        2 * APB_CLK_FREQ / host_div / if card_div == 0 { 1 } else { card_div * 2 } / 1000
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

        self.set_card_clk(slot, freq_khz).await?;
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

    pub async fn enable_clk_cmd11(&self, slot: Slot, en: bool) -> Result<(), Error> {
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
}
