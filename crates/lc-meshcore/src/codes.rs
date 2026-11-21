//! Companion-protocol codes (`oxymesh/docs/spec/companion.md` §2 and §4).

/// App commands used by lichen.
pub mod cmd {
    pub const APP_START: u8 = 1;
    pub const SEND_TXT_MSG: u8 = 2;
    pub const SEND_CHANNEL_TXT_MSG: u8 = 3;
    pub const GET_CONTACTS: u8 = 4;
    pub const GET_DEVICE_TIME: u8 = 5;
    pub const SET_DEVICE_TIME: u8 = 6;
    pub const SEND_SELF_ADVERT: u8 = 7;
    pub const SET_ADVERT_NAME: u8 = 8;
    /// A contact record, added or updated (§16.6).
    pub const ADD_UPDATE_CONTACT: u8 = 9;
    pub const SET_RADIO_PARAMS: u8 = 11;
    pub const SYNC_NEXT_MESSAGE: u8 = 10;
    pub const SET_RADIO_TX_POWER: u8 = 12;
    pub const RESET_PATH: u8 = 13;
    pub const SET_ADVERT_LATLON: u8 = 14;
    /// A node's advert, handled as if heard (§16.6).
    pub const IMPORT_CONTACT: u8 = 18;
    pub const REBOOT: u8 = 19;
    pub const GET_BATT_AND_STORAGE: u8 = 20;
    pub const DEVICE_QUERY: u8 = 22;
    pub const EXPORT_PRIVATE_KEY: u8 = 23;
    pub const SEND_RAW_DATA: u8 = 25;
    pub const SEND_LOGIN: u8 = 26;
    pub const SEND_STATUS_REQ: u8 = 27;
    pub const LOGOUT: u8 = 29;
    pub const GET_CONTACT_BY_KEY: u8 = 30;
    pub const GET_CHANNEL: u8 = 31;
    pub const SET_CHANNEL: u8 = 32;
    pub const SEND_TRACE_PATH: u8 = 36;
    pub const SET_DEVICE_PIN: u8 = 37;
    pub const SET_OTHER_PARAMS: u8 = 38;
    pub const GET_CUSTOM_VARS: u8 = 40;
    pub const SET_CUSTOM_VAR: u8 = 41;
    pub const SEND_BINARY_REQ: u8 = 50;
    pub const SEND_PATH_DISCOVERY_REQ: u8 = 52;
    pub const GET_STATS: u8 = 56;
    pub const SET_AUTOADD_CONFIG: u8 = 58;
    pub const GET_AUTOADD_CONFIG: u8 = 59;
    pub const GET_DEFAULT_FLOOD_SCOPE: u8 = 64;
    pub const SEND_RAW_PACKET: u8 = 65;
}

/// Device responses (< 0x80) and push notifications (≥ 0x80).
pub mod resp {
    pub const OK: u8 = 0;
    pub const ERR: u8 = 1;
    pub const CONTACTS_START: u8 = 2;
    pub const CONTACT: u8 = 3;
    pub const END_OF_CONTACTS: u8 = 4;
    pub const SELF_INFO: u8 = 5;
    pub const SENT: u8 = 6;
    pub const CONTACT_MSG_LEGACY: u8 = 7;
    pub const CHANNEL_MSG_LEGACY: u8 = 8;
    pub const CURR_TIME: u8 = 9;
    pub const NO_MORE_MESSAGES: u8 = 10;
    pub const BATT_AND_STORAGE: u8 = 12;
    pub const DEVICE_INFO: u8 = 13;
    pub const PRIVATE_KEY: u8 = 14;
    pub const DISABLED: u8 = 15;
    pub const CONTACT_MSG: u8 = 0x10;
    pub const CHANNEL_MSG: u8 = 0x11;
    pub const CHANNEL_INFO: u8 = 0x12;
    pub const CUSTOM_VARS: u8 = 21;
    pub const STATS: u8 = 24;
    pub const AUTOADD_CONFIG: u8 = 25;
    pub const DEFAULT_FLOOD_SCOPE: u8 = 28;

    pub const ADVERT: u8 = 0x80;
    pub const PATH_UPDATED: u8 = 0x81;
    pub const SEND_CONFIRMED: u8 = 0x82;
    pub const MSG_WAITING: u8 = 0x83;
    pub const RAW_DATA: u8 = 0x84;
    pub const LOGIN_SUCCESS: u8 = 0x85;
    pub const LOGIN_FAIL: u8 = 0x86;
    pub const STATUS_RESPONSE: u8 = 0x87;
    pub const LOG_RX_DATA: u8 = 0x88;
    pub const TRACE_DATA: u8 = 0x89;
    pub const NEW_ADVERT: u8 = 0x8A;
    pub const BINARY_RESPONSE: u8 = 0x8C;
    pub const PATH_DISCOVERY_RESPONSE: u8 = 0x8D;
}

/// Error codes (`ERR` = `[0x01, code]`).
pub mod err {
    pub const UNSUPPORTED_CMD: u8 = 1;
    pub const NOT_FOUND: u8 = 2;
    pub const TABLE_FULL: u8 = 3;
    pub const BAD_STATE: u8 = 4;
    pub const FILE_IO_ERROR: u8 = 5;
    pub const ILLEGAL_ARG: u8 = 6;
}

/// Text types of `SEND_TXT_MSG` and `CONTACT_MSG` (`src/helpers/TxtDataHelpers.h`).
pub mod txt {
    pub const PLAIN: u8 = 0;
    /// A CLI command to a server, or its reply.
    pub const CLI_DATA: u8 = 1;
    /// A room server's post: the text starts with its author's 4-byte key prefix.
    pub const SIGNED_PLAIN: u8 = 2;
}

/// Request types of `SEND_BINARY_REQ` to a server (`examples/simple_repeater/MyMesh.cpp`).
pub mod req {
    pub const GET_STATUS: u8 = 0x01;
    pub const GET_NEIGHBOURS: u8 = 0x06;
}

/// Protocol version announced by `DEVICE_QUERY`: ≥ 3 to receive V3 frames.
pub const APP_VER: u8 = 3;
