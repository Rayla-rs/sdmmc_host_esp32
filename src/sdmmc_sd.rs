use embassy_futures::yield_now;
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use embassy_time::{block_for, Duration, WithTimeout};
use embedded_sdmmc::BlockDevice;
use esp_hal::{
    dma::{DmaDescriptor, DmaRxBuf, DmaRxBuffer, DmaTxBuf},
    peripherals::SDHOST,
};
use log::{debug, info, warn};
use sdio_host::{common_cmd::Resp, sd::SD, Cmd};

pub mod cmd;
pub mod common;
pub mod init;
pub mod io;

use crate::{
    bit, cmd::SdmmcCmd, common::*, common::*, inter::Event, sdmmc::Sdmmc, Error, Slot, Width,
    EVENT_QUEUE,
};
const TAG: &'static str = "[SDMMC_CARD]";

pub struct TransState {
    ptr: *mut u8,
    size_remaining: usize,
    next_desc: usize,
    desc_remaining: usize,
}

struct CSD {
    pub(crate) sector_size: u32,
    pub(crate) capacity: u32,
}

pub struct SdmmcCard {
    sdmmc: Sdmmc,
    slot: Slot,
    width: Width,
    bus_sampling_mode: BusSamplingMode,
    freq_khz: u32, // default is 400
    dma_rx_buf: DmaRxBuf,
    dma_tx_buf: DmaTxBuf,
    rsa: u32,
    pub(crate) is_mmc: bool,
    ocr: u32,
    pub(crate) raw_cid: [u32; 4],
    pub(crate) rca: u16,
    pub(crate) csd: CSD, // look at later
}

pub struct SdmmcDevice(Mutex<CriticalSectionRawMutex, SdmmcCard>);

impl BlockDevice for SdmmcDevice {
    type Error = Error;
    fn read(
        &self,
        blocks: &mut [embedded_sdmmc::Block],
        start_block_idx: embedded_sdmmc::BlockIdx,
    ) -> Result<(), Self::Error> {
        unsafe {
            self.0.lock_mut(|card| {
                embassy_futures::block_on(card.read_sectors(blocks, start_block_idx))
            })
        } // :3
    }
    fn write(
        &self,
        blocks: &[embedded_sdmmc::Block],
        start_block_idx: embedded_sdmmc::BlockIdx,
    ) -> Result<(), Self::Error> {
        unsafe {
            self.0
                .lock_mut(|card| embassy_futures::block_on(card.write_sectors()))
        } // :3
    }
    fn num_blocks(&self) -> Result<embedded_sdmmc::BlockCount, Self::Error> {
        // unsafe {
        //     self.0
        //         .lock_mut(|card| embassy_futures::block_on())
        // } // :3
        todo!()
    }
}

impl SdmmcCard {
    pub async fn new(
        sdhost: SDHOST<'static>,
        dma_rx_buf: DmaRxBuf,
        dma_tx_buf: DmaTxBuf,
    ) -> SdmmcCard {
        let mut card = SdmmcCard {
            sdmmc: Sdmmc::new(sdhost),
            slot: Slot::Slot1,
            width: Width::Bit1,
            bus_sampling_mode: BusSamplingMode::SDR,
            freq_khz: 20000,
            dma_rx_buf,
            dma_tx_buf,
            rsa: 0,
            ocr: 0,
            raw_cid: [0u32; 4],
            rca: 0,
            csd: CSD {
                sector_size: 0,
                capacity: 0,
            },
            is_mmc: false,
        };
        card.sdmmc.init().await.unwrap();
        card
    }

    pub async fn init_sd_if_cond(&mut self) -> Result<(), Error> {
        let mut host_ocr = SD_OCR_VOL_MASK;
        match self.cmd_send_if_cond(host_ocr).await {
            Ok(_) => {
                info!("{TAG} SDHC/SDXC card");
                host_ocr |= SD_OCR_SDHC_CAP;
            }
            Err(err) => match err {
                Error::Timeout => {
                    info!("{TAG} CMD8 timeout; not an SD v2.00 card");
                }
                Error::NotSupported => {
                    info!("{TAG} CMD8 rejected; not an SD v2.00 card");
                }
                _ => {
                    warn!("{TAG} send_if_cond returned {err:?}");
                    Err(err)?;
                }
            },
        }
        self.ocr = host_ocr;
        Ok(())
    }

    fn decode_cid(&self) -> Result<(), Error> {
        let a = sdio_host::sd::CID::<[u32; 4]>::from(self.raw_cid);
        // sdio_host::emmc::CID::<[u32; 4]>::from(self.raw_cid);
        todo!()
    }

    fn mmc_decode_cid(&self) -> Result<(), Error> {
        todo!()
    }

    fn decode_csd(&self, cmd: &SdmmcCmd) -> sdio_host::sd::CSD<SD> {
        sdio_host::sd::CSD::<SD>::from(cmd.responce)
    }
}

impl SdmmcCard {
    async fn do_transaction(&mut self, cmd_info: &mut SdmmcCmd<'_>) -> Result<(), Error> {
        // NOTE critical section is not needed due to ownership
        // let block = self.sdmmc.host.register_block();

        self.sdmmc.set_card_clk(self.slot, self.freq_khz).await?;

        // TODO handle idle state events

        if cmd_info.opcode == SD_SWITCH_VOLTAGE {
            self.handle_voltage_switch_stage1(self.slot, cmd_info).await;
        }

        let hw_cmd = cmd_info.make_hw_cmd();
        if cmd_info.data.is_some() {
            if cmd_info.datalen >= 4 && cmd_info.datalen % 4 != 0 {
                warn!(
                    "{TAG} do_transaction: invalid size: total={}",
                    cmd_info.datalen
                );
                Err(Error::InvalidSize)
            } else {
                Ok(())
            }?;

            // May need to add alignment check for sanity purposes later here

            self.dma_prepare(cmd_info.datalen, cmd_info.blklen);
            // self.dma_rx_buf.
        }

        self.sdmmc
            .start_cmd(crate::Slot::Slot1, hw_cmd, cmd_info.arg)
            .await?;

        // process events until transfer is complete
        let mut ret = Ok(());
        let mut unhandled = Event {
            sdmmc_status: 0,
            dma_status: 0,
        };
        cmd_info.err = None;
        let mut state = if cmd_info.opcode == SD_SWITCH_VOLTAGE {
            State::SendingVoltageSwitch
        } else {
            State::SendingCmd
        };

        while state != State::Idle && ret.is_ok() {
            ret = self
                .handle_event(self.slot, cmd_info, &mut state, &mut unhandled)
                .await;
        }

        if ret.is_ok() && cmd_info.has_flag(SCF_WAIT_BUSY) {
            if !self.wait_for_busy_cleared(cmd_info.timeout_ms).await {
                info!("{TAG} wait_for_busy_cleared returned false");
                ret = Err(Error::Timeout);
            }
        }

        if let Some(buf) = cmd_info.data.as_mut() {
            let bytes = self.dma_rx_buf.read_received_data(buf);
            debug!("{TAG} received data with {bytes} bytes left");
        }

        ret
    }

    async fn wait_for_event(&self, ticks: u64) -> Result<Event, Error> {
        EVENT_QUEUE
            .receive()
            .with_timeout(Duration::from_ticks(ticks))
            .await
            .map_err(|_| Error::Timeout)
    }

    async fn handle_event(
        &mut self,
        slot: Slot,
        cmd: &mut SdmmcCmd<'_>,
        state: &mut State,
        unhandled: &mut Event,
    ) -> Result<(), Error> {
        match self
            .wait_for_event(Duration::from_millis(cmd.timeout_ms).as_ticks())
            .await
        {
            Ok(mut event) => {
                info!(
                    "{} handle_event: slot {:?} event {:?} unhandled {:?}",
                    TAG, slot, event, unhandled
                );
                event.sdmmc_status |= unhandled.sdmmc_status;
                event.dma_status |= unhandled.dma_status;
                self.process_events(slot, cmd, state, event, unhandled)
                    .await;
                info!(
                    "{} handle_event: slot {:?} events unhandled {:?}",
                    TAG, slot, unhandled
                );
                Ok(())
            }
            Err(err) => {
                warn!("{} wait_for_event returned {:?}", TAG, err);
                self.sdmmc.dma_stop();
                Err(err)
            }
        }
    }

    async fn process_events(
        &mut self,
        slot: Slot,
        cmd: &mut SdmmcCmd<'_>,
        pstate: &mut State,
        mut event: Event,
        unhandled: &mut Event,
    ) {
        let orig_evt = event;
        let mut next_state = *pstate;
        let mut state = State::None;
        while next_state != state {
            state = next_state;
            match state {
                State::None => {
                    unreachable!()
                }
                State::Idle => {}
                State::SendingCmd => {
                    if mask_check_and_clear(&mut event.sdmmc_status, SD_CMD_ERR_MASK) {
                        self.process_command_response(orig_evt.sdmmc_status, cmd);
                    }
                    if mask_check_and_clear(&mut event.sdmmc_status, SDMMC_INTMASK_CMD_DONE) {
                        self.process_command_response(orig_evt.sdmmc_status, cmd);

                        next_state = if cmd.err.is_some() {
                            State::Idle
                        } else if cmd.data.is_none() {
                            State::Idle
                        } else {
                            State::SendingData
                        };
                    }
                }
                State::SendingData => {
                    if mask_check_and_clear(&mut event.sdmmc_status, SD_DATA_ERR_MASK) {
                        self.process_data_status(orig_evt.sdmmc_status, cmd);
                        self.sdmmc.dma_stop();
                    }
                    if mask_check_and_clear(&mut event.dma_status, SD_DMA_DONE_MASK) {
                        next_state = State::Busy;
                    }
                    if orig_evt.sdmmc_status & (SDMMC_INTMASK_SBE | SDMMC_INTMASK_DATA_OVER) != 0 {
                        next_state = State::Idle;
                    }
                }
                State::Busy => {
                    if mask_check_and_clear(&mut event.sdmmc_status, SDMMC_INTMASK_DATA_OVER) {
                        self.process_data_status(orig_evt.sdmmc_status, cmd);
                        next_state = State::Idle;
                    }
                }
                State::SendingVoltageSwitch => {
                    if mask_check_and_clear(&mut event.sdmmc_status, SD_CMD_ERR_MASK) {
                        self.process_command_response(orig_evt.sdmmc_status, cmd);
                        next_state = State::Idle;
                    }
                    if mask_check_and_clear(&mut event.sdmmc_status, SDMMC_INTMASK_VOLT_SW) {
                        self.handle_voltage_switch_stage2(slot, cmd).await.unwrap();
                        next_state = if cmd.err.is_some() {
                            State::Idle
                        } else {
                            State::WaitingVoltageSwitch
                        };
                    }
                }
                State::WaitingVoltageSwitch => {
                    if mask_check_and_clear(&mut event.sdmmc_status, SD_CMD_ERR_MASK) {
                        self.process_command_response(orig_evt.sdmmc_status, cmd);
                        next_state = State::Idle;
                    }
                    if mask_check_and_clear(&mut event.sdmmc_status, SDMMC_INTMASK_VOLT_SW) {
                        self.handle_voltage_switch_stage3(cmd).await;
                        next_state = State::Idle;
                    }
                }
            }
            info!("{TAG} state: {state:?} next_state: {next_state:?}");
        }
        *pstate = state;
        unhandled.sdmmc_status = event.sdmmc_status;
        unhandled.dma_status = event.dma_status;
    }

    fn process_command_response(&self, status: u32, cmd: &mut SdmmcCmd) {
        if cmd.has_flag(SCF_RSP_PRESENT) {
            if cmd.has_flag(SCF_RSP_136) {
                cmd.responce[0] = self.sdmmc.host.register_block().resp0().read().bits();
                cmd.responce[1] = self.sdmmc.host.register_block().resp1().read().bits();
                cmd.responce[2] = self.sdmmc.host.register_block().resp2().read().bits();
                cmd.responce[3] = self.sdmmc.host.register_block().resp3().read().bits();
            } else {
                cmd.responce[0] = self.sdmmc.host.register_block().resp0().read().bits();
                cmd.responce[1] = 0;
                cmd.responce[2] = 0;
                cmd.responce[3] = 0;
            }
        }
        if let Some(err) = if status & SDMMC_INTMASK_RTO != 0 {
            Some(Error::Timeout)
        } else if cmd.has_flag(SCF_RSP_CRC) && status & SDMMC_INTMASK_RCRC != 0 {
            Some(Error::InvalidCRC)
        } else if status & SDMMC_INTMASK_RESP_ERR != 0 {
            Some(Error::InvalidResponce)
        } else {
            None
        } {
            cmd.err = Some(err);
            if cmd.data.is_some() {
                self.sdmmc.dma_stop();
            }
            warn!("{TAG} process_command_responce: error {err:?} status={status:b}");
        }
    }

    fn process_data_status(&self, status: u32, cmd: &mut SdmmcCmd) {
        if status & SD_DATA_ERR_MASK != 0 {
            cmd.err = Some(if status & SDMMC_INTMASK_DTO != 0 {
                warn!("{TAG} process_data_status data timeout");
                Error::Timeout
            } else if status & SDMMC_INTMASK_DCRC != 0 {
                Error::InvalidCRC
            } else if (status & SDMMC_INTMASK_EBE != 0) && !cmd.has_flag(SCF_CMD_READ) {
                Error::Timeout
            } else {
                Error::Fail
            });
            self.sdmmc
                .host
                .register_block()
                .ctrl()
                .write(|w| w.fifo_reset().set_bit());
        }
        if cmd.err.is_some() {
            if cmd.data.is_some() {
                self.dma_stop();
            }
            warn!("{TAG} process data status error {:?}", cmd.err);
        }
    }

    async fn handle_voltage_switch_stage1(&mut self, slot: Slot, cmd: &mut SdmmcCmd<'_>) {
        info!("{TAG} enabling clock");
        self.sdmmc.set_clk_always_on(slot, true).await;
    }

    async fn handle_voltage_switch_stage2(
        &mut self,
        slot: Slot,
        cmd: &mut SdmmcCmd<'_>,
    ) -> Result<(), Error> {
        info!("{TAG} disabling clock");
        self.sdmmc.enable_clk_cmd11(slot, false).await?;
        block_for(Duration::from_micros(100));

        info!("{TAG} switching voltage");
        todo!("Impl Voltage Switch");
        // maybe update GPIO13 level from 3.3v to 1.8v

        info!("{TAG} blocking for 10ms");
        block_for(Duration::from_millis(10));

        info!("{TAG} enabling clock");
        self.sdmmc.enable_clk_cmd11(slot, true).await
    }

    async fn handle_voltage_switch_stage3(&mut self, cmd: &mut SdmmcCmd<'_>) {
        info!("{TAG} voltage switch complete, clock back to lp mode");
        self.sdmmc.set_clk_always_on(self.slot, true).await;
    }

    fn set_bus_width(&self) -> Result<(), Error> {
        self.sdmmc.set_bus_width(self.slot, self.width)?;
        // match self.width {
        //     Width::Bit1 => {}
        //     _ => {
        //         // configure pin
        //     }
        // }
        Ok(())
    }

    fn set_bus_sampling_mode(&self) -> Result<(), Error> {
        if self.width == Width::Bit8 && self.bus_sampling_mode == BusSamplingMode::DDR {
            warn!("{TAG} Bus width 8 does not support DDR");
            Err(Error::InvalidArg)?;
        }

        let uhs = self.sdmmc.host.register_block().uhs();
        let emmcddr = self.sdmmc.host.register_block().emmcddr();

        Ok(match self.bus_sampling_mode {
            BusSamplingMode::SDR => {
                uhs.modify(|r, w| unsafe { w.ddr().bits(r.ddr().bits() | (self.slot as u8)) });
                emmcddr.modify(|r, w| unsafe {
                    w.halfstartbit()
                        .bits(r.halfstartbit().bits() | (self.slot as u8))
                });
            }
            BusSamplingMode::DDR => {
                uhs.modify(|r, w| unsafe { w.ddr().bits(r.ddr().bits() & !(self.slot as u8)) });
                emmcddr.modify(|r, w| unsafe {
                    w.halfstartbit()
                        .bits(r.halfstartbit().bits() & !(self.slot as u8))
                });
            }
        })
    }

    // fn get_real_freq(&self) -> u32 {
    //     let host_div = self.sdmmc.get_clock_div();
    //     let card_div = self.sdmmc.get_card_clock_div(self.slot);
    //     self.sdmmc.calc_freq(host_div, card_div)
    // }

    fn dma_prepare(&mut self, data_size: u32, block_size: u32) {
        let prep = self.dma_rx_buf.prepare();

        let block = self.sdmmc.host.register_block();
        block.bytcnt().write(|w| unsafe { w.bits(data_size) });
        block.blksiz().write(|w| unsafe { w.bits(block_size) });
        block
            .dbaddr()
            .write(|w| unsafe { w.dbaddr().bits(prep.start.addr() as u32) });
        self.sdmmc.enable_dma(true);
        self.dma_resume();
    }

    fn dma_resume(&self) {
        self.sdmmc.dma_resume();
    }

    async fn wait_for_busy_cleared(&self, timeout_ms: u64) -> bool {
        if timeout_ms == 0 {
            !self.card_busy()
        } else {
            for _ in 0..Duration::from_millis(timeout_ms).as_ticks() {
                if !self.card_busy() {
                    return true;
                }
                yield_now().await;
            }
            false
        }
    }

    fn card_busy(&self) -> bool {
        self.sdmmc
            .host
            .register_block()
            .status()
            .read()
            .data_busy()
            .bit()
    }

    fn dma_stop(&self) {
        self.sdmmc.dma_stop();
    }
}

fn mask_check_and_clear(state: &mut u32, mask: u32) -> bool {
    let ret = ((*state) & mask) != 0;
    *state &= !mask;
    ret
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum State {
    None,
    Idle,
    SendingCmd,
    SendingData,
    Busy,
    SendingVoltageSwitch,
    WaitingVoltageSwitch,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum BusSamplingMode {
    SDR = 1,
    DDR,
}
// sampling mode state
// sampling mode
