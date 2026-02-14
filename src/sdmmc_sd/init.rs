use embassy_time::Timer;
use log::trace;

use crate::{
    common::{SD_OCR_S18_RA, SD_OCR_SDHC_CAP},
    sdmmc_sd::SdmmcCard,
    Error,
};

const TAG: &'static str = "[SDMMC_INIT]";

impl SdmmcCard {
    pub async fn init(&mut self) -> Result<(), Error> {
        macro_rules! SDMMC_INIT_STEP {
            ($cond: expr, $func: ident) => {
                if $cond {
                    self.$func().await?
                }
            };
        }

        // self.is_mmc = true; // for testing
        self.sdmmc
            .init_slot(crate::Slot::Slot1, crate::Width::Bit1, &mut self.freq_khz)
            .await?;

        self.fix_host_flags().await?;

        self.check_host_function_ptr_integrity().await?;

        // SD reset - CMD0
        SDMMC_INIT_STEP!(true, cmd_go_idle_state);

        // CMD8
        SDMMC_INIT_STEP!(true, init_sd_if_cond);

        Timer::after_millis(10).await;

        // Use SEND_OP_COND to setup card OCR
        // SDMMC_INIT_STEP!(self.is_mem, init_ocr);
        SDMMC_INIT_STEP!(true, init_ocr);

        // Check for UHS-I
        let is_sdmem = true;
        let is_uhs1 = is_sdmem && self.ocr & SD_OCR_S18_RA != 0 && self.ocr & SD_OCR_SDHC_CAP != 0;
        log::info!("{TAG} is_uhs1:{is_uhs1}");

        // CMD2
        // SDMMC_INIT_STEP!(self.is_mem, init_cid);

        // CMD3
        SDMMC_INIT_STEP!(!self.host_is_spi(), init_rca);

        // SDMMC_INIT_STEP!(self.is_mem, init_csd);

        if self.is_mmc && !self.host_is_spi() {
            self.init_mmc_decode_cid()?
        };
        // CMD9
        // self.init_csd().await?;

        // if self.is_mmc {
        //     self.init_mmc_decode_cid()?;
        // }
        SDMMC_INIT_STEP!(!self.host_is_spi(), init_select_card);

        // SDMMC_INIT_STEP!(is_sdmem, init_sd_blocklen);
        // SDMMC_INIT_STEP!(is_sdmem, init_sd_scr);
        // SDMMC_INIT_STEP!(is_sdmem, init_sd_wait_data_ready);

        let buf = &mut [0u8; 512];
        self.read_sectors_dma(buf, 2, 1, 512).await?;
        trace!("{TAG} buf: {buf:?}");
        Ok(())
    }
}
