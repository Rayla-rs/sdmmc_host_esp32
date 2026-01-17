use log::{trace, warn};

use crate::{
    common::{SD_OCR_S18_RA, SD_OCR_SDHC_CAP},
    sdmmc_sd::SdmmcCard,
    Error,
};

const TAG: &'static str = "[SDMMC_INIT]";

impl SdmmcCard {
    pub async fn init(&mut self) -> Result<(), Error> {
        self.is_mmc = true; // for testing

        self.fix_host_flags().await?;

        self.check_host_function_ptr_integrity().await?;

        // TODO io_reset

        // SD reset - CMD0
        self.cmd_go_idle_state().await?;

        // CMD8
        self.init_sd_if_cond().await?;

        // CMD5
        self.init_ocr().await?;

        // Check for UHS-I
        let is_sdmem = true;
        let is_uhs1 = is_sdmem && self.ocr & SD_OCR_S18_RA != 0 && self.ocr & SD_OCR_SDHC_CAP != 0;
        log::info!("{TAG} is_uhs1:{is_uhs1}");

        // CMD2
        // self.init_cid().await?; // optional

        // CMD3
        // self.init_rca().await?;

        // CMD9
        // self.init_csd().await?;

        // if self.is_mmc {
        //     self.init_mmc_decode_cid()?;
        // }

        self.init_select_card().await?;

        let buf = &mut [0u8; 512];
        self.read_sectors_dma(buf, 2, 1, 512).await?;
        trace!("{TAG} buf: {buf:?}");
        Ok(())
    }
}
