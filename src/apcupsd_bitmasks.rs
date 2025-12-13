//! Adapted from apcupsd defines.h

pub(crate) mod status {
	/* bit values for APC UPS Status Byte (ups->Status) */
	pub(crate) const UPS_CALIBRATION: u32 = 0x00000001;
	pub(crate) const UPS_TRIM: u32 = 0x00000002;
	pub(crate) const UPS_BOOST: u32 = 0x00000004;
	pub(crate) const UPS_ONLINE: u32 = 0x00000008;
	pub(crate) const UPS_ONBATT: u32 = 0x00000010;
	pub(crate) const UPS_OVERLOAD: u32 = 0x00000020;
	pub(crate) const UPS_BATTLOW: u32 = 0x00000040;
	pub(crate) const UPS_REPLACEBATT: u32 = 0x00000080;

	/* Extended bit values added by apcupsd */
	pub(crate) const UPS_COMMLOST: u32 = 0x00000100; /* Communications with UPS lost */
	pub(crate) const UPS_SHUTDOWN: u32 = 0x00000200; /* Shutdown in progress */
	pub(crate) const UPS_SLAVE: u32 = 0x00000400; /* Set if this is a slave */
	pub(crate) const UPS_SLAVEDOWN: u32 = 0x00000800; /* Slave not responding */
	pub(crate) const UPS_ONBATT_MSG: u32 = 0x00020000; /* Set when UPS_ONBATT message is sent */
	pub(crate) const UPS_FASTPOLL: u32 = 0x00040000; /* Set on power failure to poll faster */
	pub(crate) const UPS_SHUT_LOAD: u32 = 0x00080000; /* Set when BatLoad <= percent */
	pub(crate) const UPS_SHUT_BTIME: u32 = 0x00100000; /* Set when time on batts > maxtime */
	pub(crate) const UPS_SHUT_LTIME: u32 = 0x00200000; /* Set when TimeLeft <= runtime */
	pub(crate) const UPS_SHUT_EMERG: u32 = 0x00400000; /* Set when battery power has failed */
	pub(crate) const UPS_SHUT_REMOTE: u32 = 0x00800000; /* Set when remote shutdown */
	pub(crate) const UPS_PLUGGED: u32 = 0x01000000; /* Set if computer is plugged into UPS */
	pub(crate) const UPS_BATTPRESENT: u32 = 0x04000000; /* Indicates if battery is connected */
}

pub(crate) mod dip_switch {
	pub(crate) const LOW_BATTERY_5_MIN: u8 = 0x01;
	pub(crate) const ALARM_DELAY_30_SEC: u8 = 0x02;
	pub(crate) const OUTPUT_TRANSFER_115_240_VOLTS: u8 = 0x04;
	pub(crate) const INPUT_VOLTAGE_RANGE_EXPANDED: u8 = 0x08;
}

pub(crate) mod register_one {
	pub(crate) const WAKEUP_MODE: u8 = 0x01;
	pub(crate) const BYPASS_MODE_INTERNAL_FAULT: u8 = 0x02;
	pub(crate) const ENTERING_BYPASS_MODE_COMMAND: u8 = 0x04;
	pub(crate) const IN_BYPASS_MODE_COMMAND: u8 = 0x08;
	pub(crate) const LEAVING_BYPASS_MODE: u8 = 0x10;
	pub(crate) const IN_BYPASS_MODE_MANUAL: u8 = 0x20;
	pub(crate) const READY_POWER_LOAD_COMMAND: u8 = 0x40;
	pub(crate) const READY_POWER_LOAD_COMMAND_OR_LINE: u8 = 0x80;
}

pub(crate) mod register_two {
	pub(crate) const BYPASS_MODE_FAN_FAILURE: u8 = 0x01;
	pub(crate) const FAN_FAILURE_ISOLATION_UNIT: u8 = 0x02;
	pub(crate) const BYPASS_SUPPLY_FAILURE: u8 = 0x04;
	pub(crate) const BYPASS_MODE_OUTPUT_VOLTAGE_SELECT_FAILURE: u8 = 0x08;
	pub(crate) const BYPASS_MODE_DC_IMBALANCE: u8 = 0x10;
	pub(crate) const BATTERY_DISCONNECTED: u8 = 0x20;
	pub(crate) const RELAY_FAULT_SMARTTRIM_SMARTBOOST: u8 = 0x40;
	pub(crate) const BAD_OUTPUT_VOLTAGE: u8 = 0x80;
}

pub(crate) mod register_three {
	pub(crate) const OUTPUT_UNPOWERED_LOW_BATTERY: u8 = 0x01;
	pub(crate) const NO_TRANSFER_OVERLOAD: u8 = 0x02;
	pub(crate) const RELAY_MALFUNCTION_POWER_OFF: u8 = 0x04;
	pub(crate) const SLEEP_MODE_COMMAND: u8 = 0x08;
	pub(crate) const SHUTDOWN_MODE_COMMAND: u8 = 0x10;
	pub(crate) const BATTERY_CHARGER_FAILURE: u8 = 0x20;
	pub(crate) const BYPASS_RELAY_FAILURE: u8 = 0x40;
	pub(crate) const OPERATING_TEMPERATURE_EXCEEDED: u8 = 0x80;
}
