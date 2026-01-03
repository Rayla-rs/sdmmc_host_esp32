#![allow(dead_code)]

use crate::bit;

pub const SDMMC_INTMASK_IO_SLOT1: u32 = bit!(17);
pub const SDMMC_INTMASK_IO_SLOT0: u32 = bit!(16);
pub const SDMMC_INTMASK_EBE: u32 = bit!(15);
pub const SDMMC_INTMASK_ACD: u32 = bit!(14);
pub const SDMMC_INTMASK_SBE: u32 = bit!(13);
pub const SDMMC_INTMASK_BCI: u32 = bit!(13);
pub const SDMMC_INTMASK_HLE: u32 = bit!(12);
pub const SDMMC_INTMASK_FRUN: u32 = bit!(11);
pub const SDMMC_INTMASK_HTO: u32 = bit!(10);
pub const SDMMC_INTMASK_VOLT_SW: u32 = SDMMC_INTMASK_HTO;
pub const SDMMC_INTMASK_DTO: u32 = bit!(9);
pub const SDMMC_INTMASK_RTO: u32 = bit!(8);
pub const SDMMC_INTMASK_DCRC: u32 = bit!(7);
pub const SDMMC_INTMASK_RCRC: u32 = bit!(6);
pub const SDMMC_INTMASK_RXDR: u32 = bit!(5);
pub const SDMMC_INTMASK_TXDR: u32 = bit!(4);
pub const SDMMC_INTMASK_DATA_OVER: u32 = bit!(3);
pub const SDMMC_INTMASK_CMD_DONE: u32 = bit!(2);
pub const SDMMC_INTMASK_RESP_ERR: u32 = bit!(1);
pub const SDMMC_INTMASK_CD: u32 = bit!(0);
pub const SDMMC_IDMAC_INTMASK_AI: u32 = bit!(9);
pub const SDMMC_IDMAC_INTMASK_NI: u32 = bit!(8);
pub const SDMMC_IDMAC_INTMASK_CES: u32 = bit!(5);
pub const SDMMC_IDMAC_INTMASK_DU: u32 = bit!(4);
pub const SDMMC_IDMAC_INTMASK_FBE: u32 = bit!(2);
pub const SDMMC_IDMAC_INTMASK_RI: u32 = bit!(1);
pub const SDMMC_IDMAC_INTMASK_TI: u32 = bit!(0);

pub const SD_DATA_ERR_MASK: u32 = SDMMC_INTMASK_DTO
    | SDMMC_INTMASK_DCRC
    | SDMMC_INTMASK_HTO
    | SDMMC_INTMASK_SBE
    | SDMMC_INTMASK_EBE;

pub const SD_DMA_DONE_MASK: u32 =
    SDMMC_IDMAC_INTMASK_RI | SDMMC_IDMAC_INTMASK_TI | SDMMC_IDMAC_INTMASK_NI;

pub const SD_CMD_ERR_MASK: u32 = SDMMC_INTMASK_RTO | SDMMC_INTMASK_RCRC | SDMMC_INTMASK_RESP_ERR;

pub const MMC_GO_IDLE_STATE: u8 = 0; /* R0 */
pub const MMC_SEND_OP_COND: u8 = 1; /* R3 */
pub const MMC_ALL_SEND_CID: u8 = 2; /* R2 */
pub const MMC_SET_RELATIVE_ADDR: u8 = 3; /* R1 */
pub const MMC_SWITCH: u8 = 6; /* R1B */
pub const MMC_SELECT_CARD: u8 = 7; /* R1 */
pub const MMC_SEND_EXT_CSD: u8 = 8; /* R1 */
pub const MMC_SEND_CSD: u8 = 9; /* R2 */
pub const MMC_SEND_CID: u8 = 10; /* R1 */
pub const MMC_READ_DAT_UNTIL_STOP: u8 = 11; /* R1 */
pub const MMC_STOP_TRANSMISSION: u8 = 12; /* R1B */
pub const MMC_SEND_STATUS: u8 = 13; /* R1 */
pub const MMC_SET_BLOCKLEN: u8 = 16; /* R1 */
pub const MMC_READ_BLOCK_SINGLE: u8 = 17; /* R1 */
pub const MMC_READ_BLOCK_MULTIPLE: u8 = 18; /* R1 */
pub const MMC_SEND_TUNING_BLOCK: u8 = 19; /* R1 */
pub const MMC_WRITE_DAT_UNTIL_STOP: u8 = 20; /* R1 */
pub const MMC_SET_BLOCK_COUNT: u8 = 23; /* R1 */
pub const MMC_WRITE_BLOCK_SINGLE: u8 = 24; /* R1 */
pub const MMC_WRITE_BLOCK_MULTIPLE: u8 = 25; /* R1 */
pub const MMC_ERASE_GROUP_START: u8 = 35; /* R1 */
pub const MMC_ERASE_GROUP_END: u8 = 36; /* R1 */
pub const MMC_ERASE: u8 = 38; /* R1B */
pub const MMC_APP_CMD: u8 = 55; /* R1 */

/* SD commands */
/* response type */
pub const SD_SEND_RELATIVE_ADDR: u8 = 3; /* R6 */
pub const SD_SEND_SWITCH_FUNC: u8 = 6; /* R1 */
pub const SD_SEND_IF_COND: u8 = 8; /* R7 */
pub const SD_SWITCH_VOLTAGE: u8 = 11; /* R1 */
pub const SD_ERASE_GROUP_START: u8 = 32; /* R1 */
pub const SD_ERASE_GROUP_END: u8 = 33; /* R1 */
pub const SD_READ_OCR: u8 = 58; /* R3 */
pub const SD_CRC_ON_OFF: u8 = 59; /* R1 */

/* SD application commands */
/* response type */
pub const SD_APP_SET_BUS_WIDTH: u8 = 6; /* R1 */
pub const SD_APP_SD_STATUS: u8 = 13; /* R2 */
pub const SD_APP_SEND_NUM_WR_BLOCKS: u8 = 22; /* R1 */
pub const SD_APP_OP_COND: u8 = 41; /* R3 */
pub const SD_APP_SEND_SCR: u8 = 51; /* R1 */

/* SD IO commands */
pub const SD_IO_SEND_OP_COND: u8 = 5; /* R4 */
pub const SD_IO_RW_DIRECT: u8 = 52; /* R5 */
pub const SD_IO_RW_EXTENDED: u8 = 53; /* R5 */

pub const SCF_ITSDONE: u32 = 0x0001; /*< command is complete */
// pub const  SCF_CMD(flags)  :u32 = ((flags) & 0x00f0);
pub const SCF_CMD_AC: u32 = 0x0000;
pub const SCF_CMD_ADTC: u32 = 0x0010;
pub const SCF_CMD_BC: u32 = 0x0020;
pub const SCF_CMD_BCR: u32 = 0x0030;
pub const SCF_CMD_READ: u32 = 0x0040; /*< read command (data expected) */
pub const SCF_RSP_BSY: u32 = 0x0100;
pub const SCF_RSP_136: u32 = 0x0200;
pub const SCF_RSP_CRC: u32 = 0x0400;
pub const SCF_RSP_IDX: u32 = 0x0800;
pub const SCF_RSP_PRESENT: u32 = 0x1000;

pub const SCF_RSP_R0: u32 = 0; /*< none */
pub const SCF_RSP_R1: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
pub const SCF_RSP_R1B: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX | SCF_RSP_BSY;
pub const SCF_RSP_R2: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_136;
pub const SCF_RSP_R3: u32 = SCF_RSP_PRESENT;
pub const SCF_RSP_R4: u32 = SCF_RSP_PRESENT;
pub const SCF_RSP_R5: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
pub const SCF_RSP_R5B: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX | SCF_RSP_BSY;
pub const SCF_RSP_R6: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
pub const SCF_RSP_R7: u32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
pub const SCF_WAIT_BUSY: u32 = 0x2000;

pub const MMC_R1_READY_FOR_DATA: u32 = 1 << 8; /* ready for next transfer */
pub const MMC_R1_APP_CMD: u32 = 1 << 5; /* app. commands supported */
pub const MMC_R1_SWITCH_ERROR: u32 = 1 << 7; /* switch command did not succeed */
pub const MMC_R1_CURRENT_STATE_POS: u32 = 9;
pub const MMC_R1_CURRENT_STATE_MASK: u32 = 0x1E00; /* card current state */
pub const MMC_R1_CURRENT_STATE_TRAN: u32 = 4;

pub const MMC_OCR_MEM_READY: u32 = 1 << 31; /* memory power-up status bit */
pub const MMC_OCR_ACCESS_MODE_MASK: u32 = 0x60000000; /* bits 30:29 */
pub const MMC_OCR_SECTOR_MODE: u32 = 1 << 30;
pub const MMC_OCR_BYTE_MODE: u32 = 1 << 29;
pub const MMC_OCR_3_5V_3_6V: u32 = 1 << 23;
pub const MMC_OCR_3_4V_3_5V: u32 = 1 << 22;
pub const MMC_OCR_3_3V_3_4V: u32 = 1 << 21;
pub const MMC_OCR_3_2V_3_3V: u32 = 1 << 20;
pub const MMC_OCR_3_1V_3_2V: u32 = 1 << 19;
pub const MMC_OCR_3_0V_3_1V: u32 = 1 << 18;
pub const MMC_OCR_2_9V_3_0V: u32 = 1 << 17;
pub const MMC_OCR_2_8V_2_9V: u32 = 1 << 16;
pub const MMC_OCR_2_7V_2_8V: u32 = 1 << 15;
pub const MMC_OCR_2_6V_2_7V: u32 = 1 << 14;
pub const MMC_OCR_2_5V_2_6V: u32 = 1 << 13;
pub const MMC_OCR_2_4V_2_5V: u32 = 1 << 12;
pub const MMC_OCR_2_3V_2_4V: u32 = 1 << 11;
pub const MMC_OCR_2_2V_2_3V: u32 = 1 << 10;
pub const MMC_OCR_2_1V_2_2V: u32 = 1 << 9;
pub const MMC_OCR_2_0V_2_1V: u32 = 1 << 8;
pub const MMC_OCR_1_65V_1_95V: u32 = 1 << 7;
