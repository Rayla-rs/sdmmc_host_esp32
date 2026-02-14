use embassy_time::Timer;
use log::{debug, error, info, warn};
use sdio_host::sd::CSD;

use crate::{cmd::SdmmcCmd, common::*, sdmmc_sd::SdmmcCard, Error, Width};

const TAG: &'static str = "[SDMMC_CMD]";

impl SdmmcCard {
    pub async fn send_cmd(&mut self, cmd: &mut SdmmcCmd<'_>) -> Result<(), Error> {
        if cmd.timeout_ms != 0 {
            cmd.timeout_ms = 1000;
        }

        debug!("{TAG} sending cmd {:?}", cmd);
        self.do_transaction(cmd).await.inspect_err(|err| {
            warn!("{TAG} cmd={}, sdmmc_req_run returned {:?}", cmd.opcode, err)
        })?;

        let state = MMC_R1_CURRENT_STATE!(cmd.responce);
        log::info!(
            "{TAG}, cmd responce {} {} {} {} err {:?} state {}",
            cmd.responce[0],
            cmd.responce[1],
            cmd.responce[2],
            cmd.responce[3],
            cmd.err,
            state
        );

        match cmd.err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub async fn send_app_cmd(&mut self, cmd: &mut SdmmcCmd<'_>) -> Result<(), Error> {
        let mut app_cmd = SdmmcCmd {
            opcode: MMC_APP_CMD,
            arg: 0x00100000, // MMC_ARG_RCA!(self.rca),
            flags: SCF_CMD_AC | SCF_RSP_R1,
            ..Default::default()
        };
        self.send_cmd(&mut app_cmd).await?;
        if !self.host_is_spi() && MMC_R1!(app_cmd.responce) & MMC_R1_APP_CMD == 0 {
            warn!("{TAG} card does not support APP_CMD");
            Err(Error::NotSupported)?;
        }
        self.send_cmd(cmd).await
    }

    pub async fn cmd_go_idle_state(&mut self) -> Result<(), Error> {
        let mut cmd = SdmmcCmd {
            opcode: MMC_GO_IDLE_STATE,
            flags: SCF_CMD_BC | SCF_RSP_R0,
            ..Default::default()
        };
        match self.send_cmd(&mut cmd).await {
            Ok(_) => {
                Timer::after_millis(20).await;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub async fn cmd_send_if_cond(&mut self, ocr: u32) -> Result<(), Error> {
        const PATTERN: u32 = 0xAA;

        let mut cmd = SdmmcCmd {
            opcode: SD_SEND_IF_COND,
            arg: ((((ocr & SD_OCR_VOL_MASK) != 0) as u32) << 8) | PATTERN,
            // arg: 0x0000001AA,
            // arg,
            flags: SCF_CMD_BCR | SCF_RSP_R7,
            ..Default::default()
        };
        self.send_cmd(&mut cmd).await?;

        if cmd.responce[0] & 0xFF != PATTERN {
            warn!(
                "{TAG} expected {PATTERN} received {}",
                cmd.responce[0] & 0xFF
            );
            Err(Error::InvalidResponce)
        } else {
            Ok(())
        }
    }

    pub async fn cmd_send_op_cond(&mut self, ocr: u32) -> Result<(), Error> {
        // Setup
        self.sdmmc.set_clk_always_on(self.slot, true).await;

        let res = 'main: {
            let mut cmd;

            const MAX_ERRORS: u32 = 3;
            const MAX_RETRIES: u32 = 300;
            let mut err_cnt = MAX_ERRORS;
            for _ in 0..MAX_RETRIES {
                cmd = SdmmcCmd::default();
                cmd.arg = ocr;
                cmd.flags = SCF_CMD_BCR | SCF_RSP_R3;
                match if !self.is_mmc {
                    cmd.opcode = SD_APP_OP_COND;
                    self.send_app_cmd(&mut cmd).await
                } else {
                    cmd.arg &= !MMC_OCR_ACCESS_MODE_MASK;
                    cmd.arg |= MMC_OCR_SECTOR_MODE;
                    cmd.opcode = MMC_SEND_OP_COND;
                    self.send_cmd(&mut cmd).await
                } {
                    Ok(_) => {
                        if !self.host_is_spi() {
                            if MMC_R3!(cmd.responce) & MMC_OCR_MEM_READY != 0 || ocr == 0 {
                                self.ocr = MMC_R3!(cmd.responce);
                                break 'main Ok(());
                            }
                        } else {
                            if SD_SPI_R1!(cmd.responce) & SD_SPI_R1_IDLE_STATE == 0 {
                                self.ocr = MMC_R3!(cmd.responce);
                                break 'main Ok(());
                            }
                        }
                        warn!("{TAG} ok but not ready cmd_r3={:b}", cmd.responce[0]);
                        Timer::after_millis(10).await
                    }
                    Err(err) => {
                        err_cnt -= 1;
                        if err_cnt == 0 {
                            error!("{TAG} sdmmc_send_app_cmd err {:?}", err);
                            break 'main Err(err);
                        } else {
                            info!("{TAG} ignoring err {:?}", err);
                            continue;
                        }
                    }
                };
            }
            Err(Error::Timeout)
        };

        // Cleanup
        self.sdmmc.set_clk_always_on(self.slot, false).await;
        res
    }

    pub async fn cmd_read_ocr(&mut self) -> Result<(), Error> {
        let mut cmd = SdmmcCmd {
            opcode: SD_READ_OCR,
            flags: SCF_CMD_BCR | SCF_RSP_R2,
            ..Default::default()
        };
        self.send_cmd(&mut cmd).await?;
        self.ocr = SD_SPI_R3!(cmd.responce);
        Ok(())
    }

    pub async fn cmd_all_send_cid(&mut self) -> Result<[u32; 4], Error> {
        let mut cmd = SdmmcCmd {
            opcode: MMC_ALL_SEND_CID,
            flags: SCF_CMD_BCR | SCF_RSP_R2,
            ..Default::default()
        };
        self.send_cmd(&mut cmd).await?;
        Ok(cmd.responce)
    }

    // cmd_send_cid not supported
    pub async fn cmd_set_relative_addr(&mut self) -> Result<(), Error> {
        let mut cmd = SdmmcCmd {
            opcode: SD_SEND_RELATIVE_ADDR,
            flags: SCF_CMD_BCR | SCF_RSP_R6,
            ..Default::default()
        };

        let mmc_rca = 1;
        if self.is_mmc {
            cmd.arg = MMC_ARG_RCA!(mmc_rca);
        }

        self.send_cmd(&mut cmd).await?;

        if self.is_mmc {
            self.rca = mmc_rca;
        } else {
            let mut response_rca = SD_R6_RCA!(cmd.responce);
            if response_rca == 0 {
                // Try to get another RCA value if RCA value in the previous response was 0x0000
                // The value 0x0000 is reserved to set all cards into the Stand-by State with CMD7
                self.send_cmd(&mut cmd).await?;
                response_rca = SD_R6_RCA!(cmd.responce);
            }
            self.rca = response_rca;
        }
        Ok(())
    }

    pub async fn cmd_set_blocklen<Ext>(&mut self, csd: &CSD<Ext>) -> Result<(), Error> {
        self.send_cmd(&mut SdmmcCmd {
            opcode: MMC_SET_BLOCKLEN,
            arg: self.csd.sector_size,
            flags: SCF_CMD_AC | SCF_RSP_R1,
            ..Default::default()
        })
        .await
    }

    pub async fn cmd_send_csd(&mut self) -> Result<(), Error> {
        let cmd = &mut SdmmcCmd {
            opcode: MMC_SEND_CSD,
            arg: MMC_ARG_RCA!(self.rca),
            flags: SCF_CMD_AC | SCF_RSP_R2,
            ..Default::default()
        };

        self.send_cmd(cmd).await?;

        assert!(!self.is_mmc);
        let csd = self.decode_csd(cmd);
        info!("{TAG} csd={csd:?}");
        todo!()
    }

    pub async fn cmd_select_card(&mut self, rca: u32) -> Result<(), Error> {
        let responce = if rca == 0 { 0 } else { SCF_RSP_R1 };
        self.send_cmd(&mut SdmmcCmd {
            opcode: MMC_SELECT_CARD,
            arg: MMC_ARG_RCA!(rca),
            flags: SCF_CMD_AC | responce,
            ..Default::default()
        })
        .await
    }

    pub async fn cmd_send_scr(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn cmd_set_bus_width(&mut self, width: Width) -> Result<(), Error> {
        todo!()
    }

    // only spi
    pub async fn cmd_crc_on_off(&mut self, crc_enable: bool) -> Result<(), Error> {
        todo!()
    }

    pub async fn cmd_send_status(&mut self) -> Result<u32, Error> {
        let cmd = &mut SdmmcCmd {
            opcode: MMC_SEND_STATUS,
            arg: MMC_ARG_RCA!(self.rca),
            flags: SCF_CMD_AC | SCF_RSP_R1,
            ..Default::default()
        };

        self.send_cmd(cmd).await?;

        Ok(if self.host_is_spi() {
            SD_SPI_R2!(cmd.responce)
        } else {
            MMC_R1!(cmd.responce)
        })
    }

    pub async fn cmd_num_of_written_blocks(&mut self) -> Result<usize, Error> {
        todo!()
    }
}

impl SdmmcCard {
    pub async fn write_sectors(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn write_sectors_dma(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn read_sectors(
        &mut self,
        blocks: &mut [embedded_sdmmc::Block],
        start_block_idx: embedded_sdmmc::BlockIdx,
    ) -> Result<(), Error> {
        // create dma capable buffer

        todo!()
    }

    pub async fn read_sectors_dma(
        &mut self,
        dst: &mut [u8],
        start_block: u32,
        block_count: u32,
        buffer_len: u32,
    ) -> Result<(), Error> {
        // if start_block + block_count > self.csd.capacity {
        //     Err(Error::InvalidSize)?;
        // }
        let block_size = 512; //self.csd.sector_size;
        let mut cmd = SdmmcCmd {
            opcode: if block_count == 1 {
                MMC_READ_BLOCK_SINGLE
            } else {
                MMC_READ_BLOCK_MULTIPLE
            },
            flags: SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1,
            blklen: block_size,
            data: Some(dst),
            datalen: block_count * block_size,
            buflen: buffer_len,
            arg: if self.ocr & SD_OCR_SDHC_CAP != 0 {
                start_block
            } else {
                start_block * block_size
            },
            ..Default::default()
        };

        let err = self.send_cmd(&mut cmd).await;
        let err_cmd13 = self.cmd_send_status().await;

        if err.is_err() {
            match err_cmd13 {
                Ok(status) => {
                    error!("{TAG} read_sectors_dma: send_cmd returned {err:?}, status {status}")
                }
                Err(err) => {
                    error!("{TAG} read_sectors_dma: send_cmd returned {err:?}, failed to get status ({err_cmd13:?})")
                }
            }
        }

        err
    }

    pub async fn erase_sectors(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn can_discard(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn can_trim(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn mmc_can_sanatize(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn mmc_sanitize(&mut self, timeout_ms: u32) -> Result<(), Error> {
        todo!()
    }

    pub async fn full_erase(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn sdmmc_get_status(&mut self) -> Result<(), Error> {
        todo!()
    }
}
