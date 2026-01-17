use crate::common::*;
use crate::{hw_cmd::SdmmcHwCmd, Error};

#[derive(Debug)]
pub struct SdmmcCmd<'a> {
    pub opcode: u8,
    pub arg: u32,
    pub responce: [u32; 4],
    pub data: Option<&'a mut [u8]>,
    pub datalen: u32,
    pub buflen: u32,
    pub blklen: u32,
    pub flags: u32,
    pub err: Option<Error>,
    pub timeout_ms: u64,
    pub volt_switch_cb_arg: Option<fn(*mut u8, u32) -> Result<(), Error>>,
}

impl<'a> Default for SdmmcCmd<'a> {
    fn default() -> Self {
        Self {
            opcode: 0,
            arg: 0,
            responce: [0u32; 4],
            data: None,
            datalen: 0,
            buflen: 0,
            blklen: 0,
            flags: 0,
            err: None,
            timeout_ms: 1000,
            volt_switch_cb_arg: None,
        }
    }
}

impl<'a> SdmmcCmd<'a> {
    pub const fn scf_cmd(&self) -> u32 {
        self.flags & 0x00f0
    }

    pub const fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    pub fn make_hw_cmd(&self) -> SdmmcHwCmd {
        if self.data.is_some() {
            assert!(self.datalen % self.blklen == 0)
        }
        SdmmcHwCmd::default()
            .with_cmd_index(self.opcode)
            .with_stop_abort_cmd(self.opcode == MMC_STOP_TRANSMISSION)
            .with_send_init(self.opcode == MMC_GO_IDLE_STATE)
            .with_volt_switch(self.opcode == SD_SWITCH_VOLTAGE)
            .with_wait_complete(
                self.opcode != MMC_STOP_TRANSMISSION
                    && self.opcode != MMC_GO_IDLE_STATE
                    && self.opcode != SD_SWITCH_VOLTAGE,
            )
            .with_response_expect(self.has_flag(SCF_RSP_PRESENT))
            .with_response_long(self.has_flag(SCF_RSP_PRESENT) && self.has_flag(SCF_RSP_136))
            .with_check_response_crc(self.has_flag(SCF_RSP_CRC))
            .with_data_expected(self.data.is_some())
            .with_rw(self.data.is_some() && self.has_flag(SCF_CMD_READ))
            .with_send_auto_stop(
                self.data.is_some()
                    && self.datalen > 0
                    && (self.opcode == MMC_WRITE_BLOCK_MULTIPLE
                        || self.opcode == MMC_READ_BLOCK_MULTIPLE
                        || self.opcode == MMC_WRITE_DAT_UNTIL_STOP
                        || self.opcode == MMC_READ_DAT_UNTIL_STOP),
            )
    }
}
