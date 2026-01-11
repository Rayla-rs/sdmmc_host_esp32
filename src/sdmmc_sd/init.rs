use crate::{sdmmc_sd::SdmmcCard, Error};

impl SdmmcCard {
    pub async fn init(&mut self) -> Result<(), Error> {
        self.fix_host_flags().await?;

        self.check_host_function_ptr_integrity().await?;

        // TODO io_reset

        self.cmd_go_idle_state().await?;

        self.init_sd_if_cond().await?;

        // self.init_io().await?;

        self.init_ocr().await?;

        self.init_cid().await?;

        self.init_rca().await?;

        self.init_csd().await?;

        if self.is_mmc {
            self.init_mmc_decode_cid()?;
        }

        self.init_select_card().await?;
        todo!()
    }
}
