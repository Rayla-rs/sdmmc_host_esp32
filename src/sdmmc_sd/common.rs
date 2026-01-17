use log::{error, info, warn};

use crate::{common::*, sdmmc_sd::SdmmcCard, Error};

const TAG: &'static str = "[SDMMC_COMMON]";

impl SdmmcCard {
    pub async fn init_ocr(&mut self) -> Result<(), Error> {
        let mut host_ocr = SD_OCR_VOL_MASK;

        let mut acdm41_arg = host_ocr;

        if self.ocr & SD_OCR_SDHC_CAP != 0 {
            acdm41_arg |= SD_OCR_SDHC_CAP;
        }

        let to_set_to_uhs1 = self
            .sdmmc
            .is_slot_set_to_uhs1(self.slot)
            .inspect_err(|_| error!("{TAG} failed to get slot info"))?;

        if to_set_to_uhs1 {
            acdm41_arg |= SD_OCR_S18_RA;
            acdm41_arg |= SD_OCR_XPC;
        }
        info!("{TAG} init_ocr acmd41_arg={acdm41_arg:b}");

        let res = self.cmd_send_op_cond(acdm41_arg).await;

        if res.is_err_and(|err| err == Error::Timeout) {
            info!("{TAG} send_op_cond timeout, trying MMC");
            self.is_mmc = true;
            self.cmd_send_op_cond(acdm41_arg)
                .await
                .inspect_err(|err| warn!("{TAG} send_op_comd returned {err:?}"))?;
        }

        self.cmd_read_ocr()
            .await
            .inspect_err(|err| warn!("{TAG} read_ocr returned {err:?}"))?;

        info!("{TAG} host_ocr={host_ocr} card_ocr={}", self.ocr);

        host_ocr &= self.ocr | !SD_OCR_VOL_MASK;
        info!(
            "{TAG} sdmmc_card_init: host_ocr={host_ocr} card_ocr={}",
            self.ocr
        );
        Ok(())
    }
    pub async fn init_cid(&mut self) -> Result<(), Error> {
        let raw_cid = self
            .cmd_all_send_cid()
            .await
            .inspect_err(|err| warn!("{TAG} all_send_cid returned {err:?}"))?;

        if self.is_mmc {
            Ok(self.raw_cid = raw_cid)
        } else {
            self.decode_cid()
                .inspect_err(|err| warn!("{TAG} decoding CID failed err={err:?}"))
        }
    }

    pub async fn init_rca(&mut self) -> Result<(), Error> {
        self.cmd_set_relative_addr()
            .await
            .inspect_err(|err| warn!("{TAG} init_rca: set_relative_addr returned {err:?}"))
    }

    pub fn init_mmc_decode_cid(&mut self) -> Result<(), Error> {
        self.mmc_decode_cid()
            .inspect_err(|err| warn!("{TAG} init_mmc_decode_cid: decoding CID failed {err:?}"))
    }
    pub async fn init_csd(&mut self) -> Result<(), Error> {
        // assert!(self.is_mem)

        self.cmd_send_csd()
            .await
            .inspect_err(|err| warn!("{TAG} init_csd: send_csd returned {err:?}"))?;

        let max_sdsc_capacity = u32::MAX / self.csd.sector_size + 1;
        if self.ocr & SD_OCR_SDHC_CAP == 0 && self.csd.capacity > max_sdsc_capacity {
            warn!(
                "{TAG} init_csd: SDSC card reports capacity={}. Limiting to {max_sdsc_capacity}",
                self.csd.capacity
            );
            self.csd.capacity = max_sdsc_capacity;
        }
        Ok(())
    }
    pub async fn init_select_card(&mut self) -> Result<(), Error> {
        self.cmd_select_card(self.rca as u32)
            .await
            .inspect_err(|err| warn!("{TAG} init_select_card: select_card returned {err:?}"))
    }
    pub async fn init_card_hs_mode(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn init_sd_driver_strength(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn init_sd_current_limit(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn init_sd_timing_tuning(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn init_host_bus_width(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn init_host_frequency(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn flip_byte_order(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn card_print_info(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn fix_host_flags(&mut self) -> Result<(), Error> {
        // Only supports one bit
        // todo!()
        warn!("Flags fix not implimented!");
        Ok(())
    }
    pub async fn allocate_aligned_buf(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn check_host_function_ptr_integrity(&mut self) -> Result<(), Error> {
        warn!("check_host_func_ptr_integrity ignored!");
        Ok(())
    }
    pub async fn get_erase_timeout_ms(&mut self) -> Result<(), Error> {
        todo!()
    }
    pub async fn wait_for_idle(&mut self) -> Result<(), Error> {
        todo!()
    }
}
