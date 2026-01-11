use esp_hal::{dma::DmaDescriptor, peripherals::DPORT};

use crate::{bit, hw_cmd::SdmmcHwCmd, sdmmc::Sdmmc, Slot, Width};

pub(crate) const SDMMC_LL_EVENT_IO_SLOT1: u32 = 1 << 17;
pub(crate) const SDMMC_LL_EVENT_IO_SLOT0: u32 = 1 << 16;
pub(crate) const SDMMC_LL_EVENT_EBE: u32 = 1 << 15;
pub(crate) const SDMMC_LL_EVENT_ACD: u32 = 1 << 14;
pub(crate) const SDMMC_LL_EVENT_SBE: u32 = 1 << 13;
pub(crate) const SDMMC_LL_EVENT_BCI: u32 = 1 << 13;
pub(crate) const SDMMC_LL_EVENT_HLE: u32 = 1 << 12;
pub(crate) const SDMMC_LL_EVENT_FRUN: u32 = 1 << 11;
pub(crate) const SDMMC_LL_EVENT_HTO: u32 = 1 << 10;
pub(crate) const SDMMC_LL_EVENT_DTO: u32 = 1 << 9;
pub(crate) const SDMMC_LL_EVENT_RTO: u32 = 1 << 8;
pub(crate) const SDMMC_LL_EVENT_DCRC: u32 = 1 << 7;
pub(crate) const SDMMC_LL_EVENT_RCRC: u32 = 1 << 6;
pub(crate) const SDMMC_LL_EVENT_RXDR: u32 = 1 << 5;
pub(crate) const SDMMC_LL_EVENT_TXDR: u32 = 1 << 4;
pub(crate) const SDMMC_LL_EVENT_DATA_OVER: u32 = 1 << 3;
pub(crate) const SDMMC_LL_EVENT_CMD_DONE: u32 = 1 << 2;
pub(crate) const SDMMC_LL_EVENT_RESP_ERR: u32 = 1 << 1;
pub(crate) const SDMMC_LL_EVENT_CD: u32 = 1 << 0;

// Default enabled interrupts (sdio is enabled only when use):
pub(crate) const SDMMC_LL_EVENT_DEFAULT: u32 = SDMMC_LL_EVENT_CD
    | SDMMC_LL_EVENT_RESP_ERR
    | SDMMC_LL_EVENT_CMD_DONE
    | SDMMC_LL_EVENT_DATA_OVER
    | SDMMC_LL_EVENT_RCRC
    | SDMMC_LL_EVENT_DCRC
    | SDMMC_LL_EVENT_RTO
    | SDMMC_LL_EVENT_DTO
    | SDMMC_LL_EVENT_HTO
    | SDMMC_LL_EVENT_HLE
    | SDMMC_LL_EVENT_SBE
    | SDMMC_LL_EVENT_EBE;

pub(crate) const SDMMC_LL_SD_EVENT_MASK: u32 = SDMMC_LL_EVENT_CD
    | SDMMC_LL_EVENT_RESP_ERR
    | SDMMC_LL_EVENT_CMD_DONE
    | SDMMC_LL_EVENT_DATA_OVER
    | SDMMC_LL_EVENT_TXDR
    | SDMMC_LL_EVENT_RXDR
    | SDMMC_LL_EVENT_RCRC
    | SDMMC_LL_EVENT_DCRC
    | SDMMC_LL_EVENT_RTO
    | SDMMC_LL_EVENT_DTO
    | SDMMC_LL_EVENT_HTO
    | SDMMC_LL_EVENT_FRUN
    | SDMMC_LL_EVENT_HLE
    | SDMMC_LL_EVENT_SBE
    | SDMMC_LL_EVENT_ACD
    | SDMMC_LL_EVENT_EBE;

// DMA interrupts (idsts register)
// pub(crate) const SDMMC_LL_EVENT_DMA_TI: u32 = SDMMC_IDMAC_INTMASK_TI;
// pub(crate) const SDMMC_LL_EVENT_DMA_RI: u32 = SDMMC_IDMAC_INTMASK_RI;
// pub(crate) const SDMMC_LL_EVENT_DMA_NI: u32 = SDMMC_IDMAC_INTMASK_NI;
// pub(crate) const SDMMC_LL_EVENT_DMA_MASK: u32 = 0x1f; //NI and AI will be indicated by TI/RI and FBE/DU respectively

impl Sdmmc {
    pub(crate) fn ll_enable_bus_clk(&self, en: bool) {
        unsafe { DPORT::steal() }
            .register_block()
            .peri_rst_en()
            .modify(|r, w| unsafe { w.peri_rst_en().bits(r.peri_rst_en().bits() | bit!(20)) });
        unsafe { DPORT::steal() }
            .register_block()
            .peri_clk_en()
            .modify(|r, w| unsafe { w.peri_clk_en().bits(r.peri_clk_en().bits() | bit!(20)) });
    }

    pub(crate) fn ll_reset_register(&self) {
        unsafe { DPORT::steal() }
            .register_block()
            .wifi_rst_en()
            .write(|w| w.sdio_host_rst().set_bit());
        unsafe { DPORT::steal() }
            .register_block()
            .wifi_rst_en()
            .write(|w| w.sdio_host_rst().clear_bit());
    }

    pub(crate) fn ll_select_clk_src(&self) {
        // leave for compatibility
    }

    // WARN
    pub(crate) fn ll_set_clk_div(&self, div: u8) {
        assert!(div > 1 && div <= 16);
        let h = div - 1;
        let l = div / 2 - 1;

        self.host.register_block().clk_edge_sel().write(|w| unsafe {
            w.ccllkin_edge_h().bits(h as u8);
            w.ccllkin_edge_l().bits(l as u8);
            w.ccllkin_edge_n().bits(h as u8)
        });
    }

    pub(crate) fn ll_deinit_clk(&self) {
        todo!()
    }

    pub(crate) fn ll_get_clk_div(&self) -> u8 {
        self.host
            .register_block()
            .clk_edge_sel()
            .read()
            .ccllkin_edge_h()
            .bits()
            + 1
    }

    pub(crate) fn ll_init_phase_delay(&self) {
        self.host.register_block().clk_edge_sel().write(|w| unsafe {
            w.cclkin_edge_drv_sel().bits(4);
            w.cclkin_edge_sam_sel().bits(4);
            w.cclkin_edge_slf_sel().bits(0)
        });
    }

    pub(crate) fn ll_enable_card_clk(&self, slot: Slot, en: bool) {
        self.host.register_block().clkena().modify(|r, w| unsafe {
            w.cclk_enable().bits(if en {
                r.cclk_enable().bits() | slot.bit()
            } else {
                r.cclk_enable().bits() & !slot.bit()
            })
        });
    }

    pub(crate) fn ll_set_card_clk_div(&self, slot: Slot, div: u8) {
        self.host.register_block().clksrc().modify(|r, w| unsafe {
            w.clksrc().bits(match slot {
                Slot::Slot0 => r.clksrc().bits() & 0b1100, // set bits 0:1 to 00
                Slot::Slot1 => (r.clksrc().bits() & 0b0011) | 0b0100, // set bits 2:3 to 01
            })
        });

        self.host.register_block().clkdiv().write(|w| unsafe {
            match slot {
                Slot::Slot0 => w.clk_divider0().bits(div),
                Slot::Slot1 => w.clk_divider1().bits(div),
            }
        });
    }

    pub(crate) fn ll_get_card_clk_div(&self, slot: Slot) -> u8 {
        match slot {
            Slot::Slot0 => self
                .host
                .register_block()
                .clkdiv()
                .read()
                .clk_divider0()
                .bits(),
            Slot::Slot1 => self
                .host
                .register_block()
                .clkdiv()
                .read()
                .clk_divider1()
                .bits(),
        }
    }

    pub(crate) fn ll_enable_card_clk_low_power(&self, slot: Slot, en: bool) {
        self.host.register_block().clkena().modify(|r, w| unsafe {
            w.lp_enable().bits(if en {
                r.lp_enable().bits() | slot.bit()
            } else {
                r.lp_enable().bits() & !slot.bit()
            })
        });
    }

    pub(crate) fn ll_reset_controller(&self) {
        self.host
            .register_block()
            .ctrl()
            .write(|w| w.controller_reset().set_bit());
    }

    pub(crate) fn ll_is_controller_reset_done(&self) -> bool {
        self.host
            .register_block()
            .ctrl()
            .read()
            .controller_reset()
            .bit_is_clear()
    }

    pub(crate) fn ll_reset_dma(&self) {
        self.host
            .register_block()
            .ctrl()
            .write(|w| w.dma_reset().set_bit());
    }

    pub(crate) fn ll_is_dma_reset_done(&self) -> bool {
        self.host
            .register_block()
            .ctrl()
            .read()
            .dma_reset()
            .bit_is_clear()
    }

    pub(crate) fn ll_reset_fifo(&self) {
        self.host
            .register_block()
            .ctrl()
            .write(|w| w.fifo_reset().set_bit());
    }

    pub(crate) fn ll_is_fifo_reset_done(&self) -> bool {
        self.host
            .register_block()
            .ctrl()
            .read()
            .fifo_reset()
            .bit_is_clear()
    }

    pub(crate) fn ll_set_data_timeout(&self, timeout_cycles: u32) {
        self.host
            .register_block()
            .tmout()
            .write(|w| unsafe { w.data_timeout().bits(timeout_cycles.min(0xffffff)) });
    }

    pub(crate) fn ll_set_responce_timeout(&self, timeout_cycles: u8) {
        self.host
            .register_block()
            .tmout()
            .write(|w| unsafe { w.response_timeout().bits(timeout_cycles) });
    }

    pub(crate) fn ll_is_card_detected(&self, slot: Slot) -> bool {
        self.host
            .register_block()
            .cdetect()
            .read()
            .card_detect_n()
            .bits()
            & slot.bit()
            == 0
    }

    pub(crate) fn ll_is_card_write_protected(&self, slot: Slot) -> bool {
        self.host
            .register_block()
            .wrtprt()
            .read()
            .write_protect()
            .bits()
            & slot.bit()
            != 0
    }

    pub(crate) fn ll_enable_1v8_mode(&self, slot: Slot, en: bool) {
        // for compatibility
    }

    pub(crate) fn ll_enable_ddr_mode(&self, slot: Slot, en: bool) {
        todo!()
    }

    pub(crate) fn ll_set_data_transfer_len(&self, len: u32) {
        todo!()
    }

    pub(crate) fn ll_set_block_size(&self, block_size: u32) {
        todo!()
    }

    pub(crate) fn ll_set_desc_addr(&self, desc: *mut DmaDescriptor) {
        todo!()
    }

    pub(crate) fn ll_poll_demand(&self) {
        todo!()
    }

    pub(crate) fn ll_set_cmd(&self, cmd: SdmmcHwCmd) {
        self.host
            .register_block()
            .cmd()
            .write(|w| unsafe { w.bits(cmd.0) });
    }

    pub(crate) fn ll_is_command_taken(&self) -> bool {
        self.host
            .register_block()
            .cmd()
            .read()
            .start_cmd()
            .bit_is_clear()
    }

    pub(crate) fn ll_set_cmd_arg(&self, arg: u32) {
        self.host
            .register_block()
            .cmdarg()
            .write(|w| unsafe { w.cmdarg().bits(arg) });
    }

    pub(crate) fn ll_get_version_id(&self) -> u32 {
        self.host.register_block().verid().read().versionid().bits()
    }

    pub(crate) fn ll_get_hw_config_info(&self) -> u32 {
        self.host.register_block().hcon().read().bits()
    }

    pub(crate) fn ll_set_card_width(&self, slot: Slot, width: Width) {
        todo!()
    }

    pub(crate) fn ll_is_card_data_busy(&self) -> bool {
        todo!()
    }

    pub(crate) fn ll_init_dma(&self) {
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

    pub(crate) fn ll_enable_dma(&self) {
        todo!()
    }

    pub(crate) fn ll_stop_dma(&self) {
        todo!()
    }

    pub(crate) fn ll_get_intr_status(&self) -> u32 {
        todo!()
    }

    pub(crate) fn ll_enable_interrupt(&self, mask: u32, en: bool) {
        self.host.register_block().intmask().modify(|r, w| unsafe {
            w.bits(if en {
                r.bits() | mask
            } else {
                r.bits() & !mask
            })
        });
    }

    pub(crate) fn ll_get_interrupt_raw(&self) -> u32 {
        todo!()
    }

    pub(crate) fn ll_clear_interrupt(&self, mask: u32) {
        self.host
            .register_block()
            .rintsts()
            .write(|w| unsafe { w.bits(mask) });
    }

    pub(crate) fn ll_enable_global_interrupt(&self, en: bool) {
        self.host
            .register_block()
            .ctrl()
            .write(|w| w.int_enable().variant(en));
    }

    pub(crate) fn ll_enable_busy_clear_interrupt(&self, en: bool) {
        self.host
            .register_block()
            .cardthrctl()
            .write(|w| w.cardclrinten().variant(en));
    }

    pub(crate) fn ll_get_idsts_interrupt_raw(&self) -> u32 {
        todo!()
    }

    pub(crate) fn ll_clear_idsts_interrupt(&self, mask: u32) {
        todo!()
    }
}
