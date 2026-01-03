use embassy_time::Timer;
use log::{info, warn};
use sdio_host::sd::CSD;

use crate::{cmd::SdmmcCmd, common::*, sdmmc_sd::SdmmcCard, Error, Width};

const TAG: &'static str = "[SDMMC_CMD]";

impl SdmmcCard {
    pub async fn send_cmd(&mut self, cmd: &mut SdmmcCmd) -> Result<(), Error> {
        cmd.timeout_ms = 1000;
        info!("{TAG} sending cmd {:?}", cmd);
        self.do_transaction(cmd).await?;
        let block = self.sdmmc.host.register_block();
        let state = (block.resp0().read().bits() >> 9) & 0xf;
        log::info!(
            "{TAG}, cmd responce {} {} {} {} err {:?} state {}",
            block.resp0().read().bits(),
            block.resp1().read().bits(),
            block.resp2().read().bits(),
            block.resp3().read().bits(),
            cmd.err,
            state
        );
        Ok(())
    }

    pub async fn send_app_cmd(&mut self, cmd: &mut SdmmcCmd) -> Result<(), Error> {
        let mut app_cmd = SdmmcCmd {
            opcode: MMC_APP_CMD,
            arg: self.rsa << 16,
            flags: SCF_CMD_AC | SCF_RSP_R1,
            ..Default::default()
        };
        self.send_cmd(&mut app_cmd).await?;
        if app_cmd.responce[0] & MMC_R1_APP_CMD == 0 {
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
        const SD_OCR_VOL_MASK: u32 = 0xFF8000;

        let mut cmd = SdmmcCmd {
            opcode: SD_SEND_IF_COND,
            arg: ((((ocr & SD_OCR_VOL_MASK) != 0) as u32) << 8) | PATTERN,
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

    pub async fn cmd_send_op_cond(&mut self, ocr: u32, ocrp: &mut u32) -> Result<(), Error> {
        // Setup
        self.sdmmc.set_clk_always_on(self.slot, true);

        let res = 'main: {
            let mut cmd;

            const MAX_ERRORS: u32 = 3;
            const MAX_RETRIES: u32 = 300;
            let mut err_cnt = MAX_ERRORS;
            for _ in 0..MAX_RETRIES {
                cmd = SdmmcCmd::default();
                cmd.arg = ocr;
                cmd.flags = SCF_CMD_BCR | SCF_RSP_R3;
                match if self.is_mmc {
                    cmd.opcode = SD_APP_OP_COND;
                    self.send_app_cmd(&mut cmd).await
                } else {
                    cmd.arg &= !MMC_OCR_ACCESS_MODE_MASK;
                    cmd.arg |= MMC_OCR_SECTOR_MODE;
                    cmd.opcode = MMC_SEND_OP_COND;
                    self.send_cmd(&mut cmd).await
                } {
                    Ok(_) => {
                        if cmd.responce[0] & MMC_OCR_MEM_READY != 0 || ocr == 0 {
                            *ocrp = cmd.responce[0];
                            break 'main Ok(());
                        }
                        Timer::after_millis(10).await
                    }
                    Err(err) => {
                        err_cnt -= 1;
                        if err_cnt == 0 {
                            warn!("{TAG} sdmmc_send_app_cmd err {:?}", err);
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
        self.sdmmc.set_clk_always_on(self.slot, false);
        res
    }

    pub async fn cmd_read_ocr(&mut self, ocrp: &mut u32) -> Result<(), Error> {
        let mut cmd = SdmmcCmd {
            opcode: SD_READ_OCR,
            flags: SCF_CMD_BCR | SCF_RSP_R2,
            ..Default::default()
        };
        self.send_cmd(&mut cmd).await?;
        *ocrp = cmd.responce[0];
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
    pub async fn cmd_set_relative_addr(&mut self, out_rca: &mut u16) -> Result<(), Error> {
        let mut cmd = SdmmcCmd {
            opcode: SD_SEND_RELATIVE_ADDR,
            flags: SCF_CMD_BCR | SCF_RSP_R6,
            ..Default::default()
        };

        let mmc_rca = 1;
        if self.is_mmc {
            cmd.arg = mmc_rca << 16;
        }

        self.send_cmd(&mut cmd).await?;

        if self.is_mmc {
            *out_rca = mmc_rca as u16;
        } else {
            let mut response_rca = cmd.responce[0] >> 16;
            if response_rca == 0 {
                self.send_cmd(&mut cmd).await?;
                response_rca = cmd.responce[0] >> 16;
            }
            *out_rca = response_rca as u16;
        }
        Ok(())
    }

    pub async fn cmd_set_blocklen<Ext>(&mut self, csd: &CSD<Ext>) -> Result<(), Error> {
        self.send_cmd(&mut SdmmcCmd {
            opcode: MMC_SET_BLOCKLEN,
            arg: todo!(),
            flags: SCF_CMD_AC | SCF_RSP_R1,
            ..Default::default()
        })
        .await
    }

    pub async fn cmd_send_csd(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn cmd_select_card(&mut self, rca: u32) -> Result<(), Error> {
        todo!()
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
        todo!()
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

    pub async fn read_sectors(&mut self) -> Result<(), Error> {
        todo!()
    }

    pub async fn read_sectors_dma(&mut self) -> Result<(), Error> {
        todo!()
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
