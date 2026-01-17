use crate::{common::*, sdmmc_sd::SdmmcCard, Error};

const TAG: &'static str = "[SDMMC_IO]";

type CisFunc = fn(*const u8, *mut u8, ()) -> Result<(), Error>;

struct CisTup {
    code: u32,
    name: &'static str,
    func: CisFunc,
}

impl SdmmcCard {
    pub async fn init_io(&mut self) -> Result<(), Error> {
        // new io file :3
        Ok(())
    }
}
