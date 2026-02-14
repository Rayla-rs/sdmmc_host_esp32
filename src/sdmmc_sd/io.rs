use log::{debug, error, warn};

use crate::{cmd::SdmmcCmd, common::*, sdmmc_sd::SdmmcCard, Error};

const TAG: &'static str = "[SDMMC_IO]";

type CisFunc = fn(*const u8, *mut u8, ()) -> Result<(), Error>;

struct CisTup {
    code: u32,
    name: &'static str,
    func: CisFunc,
}

impl SdmmcCard {
    pub async fn init_io(&mut self) -> Result<(), Error> {
        match self.io_send_op_cond(0).await {
            Ok(_) => {
                self.is_mem = if self.ocr & SD_IO_OCR_MEM_PRESENT != 0 {
                    debug!("{TAG} init_io: Combination card");
                    true
                } else {
                    debug!("{TAG} init_io: IO-only card");
                    false
                };

                self.num_io_functions = sd_io_ocr_num_functions(self.ocr);
                debug!(
                    "{TAG} init_io: number of IO functions: {}",
                    self.num_io_functions
                );
                self.is_sdio = self.num_io_functions != 0;
                let host_ocr = SD_OCR_VOL_MASK & self.ocr;
                self.io_send_op_cond(host_ocr).await.inspect_err(|err| {
                    error!("{TAG} init_io: io_send_op_cond (1) returned {err:?}")
                })?;

                if let Err(err) = self.io_enable_int().await {
                    debug!("{TAG} init_io: enable_int failed {err:?}");
                }
            }

            Err(err) => {
                debug!("{TAG} init_io: io_send_op_cond (1) returned {err:?}; not IO card");
                self.is_sdio = false;
                self.is_mem = true;
            }
        }

        Ok(())
    }

    pub async fn io_enable_int(&mut self) -> Result<(), Error> {
        self.sdmmc.io_int_enable(self.slot)
    }

    pub async fn io_send_op_cond(&mut self, ocr: u32) -> Result<(), Error> {
        let cmd = &mut SdmmcCmd {
            opcode: SD_IO_SEND_OP_COND,
            arg: ocr,
            flags: SCF_CMD_BCR | SCF_RSP_R4,
            ..Default::default()
        };

        'send: {
            const SDMMC_IO_SEND_OP_COND_DELAY_MS: u64 = 10;

            for _ in 0..100 {
                self.send_cmd(cmd).await?;

                if cmd.responce[0] & SD_IO_OCR_MEM_READY != 0 || ocr == 0 {
                    self.ocr = cmd.responce[0];
                    break 'send Ok(());
                }

                embassy_time::Timer::after_millis(SDMMC_IO_SEND_OP_COND_DELAY_MS).await
            }
            Err(Error::Timeout)
        }
    }

    pub async fn io_reset(&mut self) -> Result<(), Error> {
        match self
            .io_rw_direct(
                0,
                SD_IO_CCCR_CTL,
                SD_ARG_CMD52_WRITE,
                &mut (CCCR_CTL_RES as u8),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                if err == Error::Timeout || err == Error::InvalidCRC {
                    Ok(())
                } else {
                    if err == Error::NotFound {
                        debug!("{TAG} io_reset: card not present")
                    } else {
                        error!("{TAG} io_reset: unexpected return: {err:?}")
                    }
                    Err(err)
                }
            }
        }
    }

    pub async fn io_rw_direct(
        &mut self,
        func: u32,
        reg: u32,
        arg: u32,
        byte: &mut u8,
    ) -> Result<(), Error> {
        let cmd = &mut SdmmcCmd {
            opcode: SD_IO_RW_DIRECT,
            arg: arg
                | (func & SD_ARG_CMD52_FUNC_MASK) << SD_ARG_CMD52_FUNC_SHIFT
                | (reg & SD_ARG_CMD52_REG_MASK) << SD_ARG_CMD52_REG_SHIFT
                | ((*byte as u32) & SD_ARG_CMD52_DATA_MASK) << SD_ARG_CMD52_DATA_SHIFT,
            flags: SCF_CMD_AC | SCF_RSP_R5,
            ..Default::default()
        };

        self.send_cmd(cmd)
            .await
            .inspect_err(|err| warn!("{TAG} io_rw_direct: send_cmd returned {err:?}"))?;

        *byte = (cmd.responce[0] & 0xff) as u8;

        Ok(())
    }
}
