use std::{
	collections::HashMap,
	fs,
	net::SocketAddr,
	ops::BitAnd,
	sync::Arc,
	time::{Duration, Instant},
};

use apcaccess::{APCAccess, APCAccessConfig};
use chrono::{DateTime, NaiveDate, NaiveTime};
use clap::Parser;
use num::{Num, Unsigned};
use prometheus_exporter_base::{
	prelude::{Authorization, ServerOptions, TlsOptions},
	render_prometheus, MetricType, MissingValue, PrometheusInstance, PrometheusMetric,
};
use serde::Deserialize;
use thiserror::Error;
use tokio::{sync::Mutex, task::spawn_blocking};

mod apcupsd_bitmasks;

const DEFAULT_CONFIG_PATH: &str = "/etc/prometheus/apcupsd_exporter_config.yaml";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let cmd_options = CommandlineOptions::parse();
	let server_options = (|| -> Result<ApcupsdExporterOptions, Box<dyn std::error::Error>> {
		if fs::exists(&cmd_options.config)? {
			Ok(serde_ignored::deserialize(
				serde_yaml::Deserializer::from_reader(fs::File::open(&cmd_options.config)?),
				|path| eprintln!("Ignoring unknown configuration key {path}"),
			)?)
		} else {
			if cmd_options.config != DEFAULT_CONFIG_PATH {
				eprintln!("Specified config file ({}) does not exist, using default values", &cmd_options.config);
			}
			Ok(Default::default())
		}
	})()?;

	let mut apc = APCThrottledAccess::new(APCAccessConfig { timeout: Duration::from_millis(500), ..Default::default() }, Duration::from_secs(1));

	render_prometheus(server_options.into(), (), |_request, _| async move {
		let data = apc.fetch().await.map_err(|e| format!("error fetching data from apcupsd: {e}\n"))?;

		let rendered_result = render_metrics(data);

		Ok(rendered_result?)
	})
	.await;

	Ok(())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CommandlineOptions {
	/// Path to configuration file
	#[arg(short, long, default_value = DEFAULT_CONFIG_PATH)]
	config: String,
}

#[derive(Deserialize)]
#[serde(default)]
struct ApcupsdExporterOptions {
	pub address: SocketAddr,
	#[serde(default)]
	pub authorization: Authorization,
	#[serde(default)]
	pub tls_options: Option<TlsOptions>,
}

impl Default for ApcupsdExporterOptions {
	fn default() -> Self {
		ApcupsdExporterOptions {
			address: SocketAddr::new([127, 0, 0, 1].into(), 9175),
			authorization: Default::default(),
			tls_options: Default::default(),
		}
	}
}

impl From<ApcupsdExporterOptions> for ServerOptions {
	fn from(val: ApcupsdExporterOptions) -> Self {
		ServerOptions {
			addr: val.address,
			authorization: val.authorization,
			tls_options: val.tls_options,
		}
	}
}

fn prometheus_instance_with_labels<N: Num + std::fmt::Display + std::fmt::Debug>(
	labels: &Vec<(String, String)>,
) -> PrometheusInstance<'_, N, MissingValue> {
	let mut instance = PrometheusInstance::new();
	for (key, val) in labels {
		instance = instance.with_label(key.as_ref(), val.as_ref());
	}
	instance
}

fn render_metrics(mut apcupsd_data: HashMap<String, String>) -> Result<String, RenderMetricsError> {
	let mut rendered = String::new();

	let mut labels = Vec::new();
	let label_keys = [("UPSNAME", "ups_name"), ("MODEL", "model"), ("SERIALNO", "serial_number")];
	for (key, label) in label_keys {
		if let Some(val) = apcupsd_data.remove(key) {
			labels.push((
				label.to_string(),
				val,
			));
		}
	}

	let info_keys = [
		("HOSTNAME", "hostname"),
		("VERSION", "version"),
		("CABLE", "cable"),
		("DRIVER", "driver"),
		("UPSMODE", "ups_mode"),
		("SHARE", "sharenet_name"),
		("MASTER", "master_name"),
		("SENSE", "sensitivity"),
		("ALARMDEL", "alarm_delay"),
		("LASTXFER", "last_transfer_reason"),
		("SELFTEST", "last_self_test_result"),
		("STESTI", "self_test_interval"),
		("MANDATE", "manufacture_date"),
		("FIRMWARE", "firmware_version"),
	];

	let mut info = prometheus_instance_with_labels(&labels).with_value(1);
	for (key, label) in info_keys {
		if let Some(val) = apcupsd_data.get(key) {
			info = info.with_label(label, val.as_str());
		}
	}
	rendered += &PrometheusMetric::build()
		.with_name("apcupsd_info")
		.with_help("Metadata for apcupsd.")
		.with_metric_type(MetricType::Gauge)
		.build()
		.render_and_append_instance(&info)
		.render();

	for (key, _) in info_keys {
		apcupsd_data.remove(key);
	}

	let mut renderer = MetricRenderer::new(labels, apcupsd_data);

	rendered += &renderer.render_metric(
		"DATE",
		MetricParseType::Timestamp,
		"apcupsd_last_update_timestamp_seconds",
		"Date and time of last update from UPS.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"STARTTIME",
		MetricParseType::Timestamp,
		"apcupsd_start_timestamp_seconds",
		"Date and time apcupsd was started.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MASTERUPD",
		MetricParseConfig {
			parse_type: MetricParseType::Timestamp,
			special_values: [("No connection to Master", None)].into(),
		},
		"apcupsd_master_update_timestamp_seconds",
		"Last time the master sent an update to the slave.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LINEV",
		MetricParseType::Voltage,
		"apcupsd_line_volts",
		"Current input line voltage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LOADPCT",
		MetricParseType::Percentage,
		"apcupsd_ups_load_percent",
		"Percentage of UPS load capacity used.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LOADAPNT",
		MetricParseType::Percentage,
		"apcupsd_ups_load_apparent_power_percent",
		"Percentage of UPS load apparent power capacity used.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"BCHARGE",
		MetricParseType::Percentage,
		"apcupsd_battery_charge_percent",
		"Current battery capacity charge percentage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"TIMELEFT",
		MetricParseType::Duration,
		"apcupsd_battery_time_left_seconds",
		"Remaining runtime left on battery as estimated by the UPS.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MBATTCHG",
		MetricParseType::Percentage,
		"apcupsd_battery_charge_required_for_shutdown_percent",
		"Min battery charge % (BCHARGE) required for system shutdown.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MINTIMEL",
		MetricParseType::Duration,
		"apcupsd_battery_runtime_required_for_shutdown_seconds",
		"Min battery runtime required for system shutdown.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MAXTIME",
		MetricParseType::Duration,
		"apcupsd_battery_runtime_trigger_shutdown_seconds",
		"Max battery runtime after which system is shutdown.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MAXLINEV",
		MetricParseType::Voltage,
		"apcupsd_max_since_startup_volts",
		"Maximum input line voltage since apcupsd startup.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"MINLINEV",
		MetricParseType::Voltage,
		"apcupsd_min_since_startup_volts",
		"Minimum input line voltage since apcupsd startup.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"OUTPUTV",
		MetricParseType::Voltage,
		"apcupsd_output_volts",
		"Current UPS output voltage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"DWAKE",
		MetricParseType::Duration,
		"apcupsd_power_on_delay_seconds",
		"Time UPS waits after power off when the power is restored.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"DSHUTD",
		MetricParseType::Duration,
		"apcupsd_power_off_delay_seconds",
		"Delay before UPS powers down after command received.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"DLOWBATT",
		MetricParseType::Duration,
		"apcupsd_battery_low_signal_time_left_seconds",
		"Low battery signal sent when this much runtime remains.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LOTRANS",
		MetricParseType::Voltage,
		"apcupsd_transfer_low_volts",
		"Input line voltage below which UPS will switch to battery.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"HITRANS",
		MetricParseType::Voltage,
		"apcupsd_transfer_high_volts",
		"Input line voltage above which UPS will switch to battery.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"RETPCT",
		MetricParseType::Percentage,
		"apcupsd_power_on_required_charge_percent",
		"Battery charge % required after power off to restore power.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"ITEMP",
		MetricParseType::Temperature,
		"apcupsd_internal_temperature_celsius",
		"UPS internal temperature in degrees Celcius.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"BATTV",
		MetricParseType::Voltage,
		"apcupsd_battery_volts",
		"Current battery voltage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LINEFREQ",
		MetricParseType::Frequency,
		"apcupsd_line_frequency_hertz",
		"Current line frequency in Hertz.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"OUTCURNT",
		MetricParseType::Current,
		"apcupsd_output_current_amps",
		"Output current in Amps.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NUMXFERS",
		MetricParseType::Count,
		"apcupsd_battery_number_transfers_total",
		"Number of transfers to battery since apcupsd startup.",
		MetricType::Counter,
	)?;
	rendered += &renderer.render_metric(
		"XONBATT",
		MetricParseType::Timestamp,
		"apcupsd_last_transfer_on_battery_timestamp_seconds",
		"Date, time of last transfer to battery since apcupsd startup.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"TONBATT",
		MetricParseType::Duration,
		"apcupsd_battery_time_on_seconds",
		"Seconds currently on battery.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"CUMONBATT",
		MetricParseType::Duration,
		"apcupsd_battery_cumulative_time_on_seconds_total",
		"Cumulative seconds on battery since apcupsd startup.",
		MetricType::Counter,
	)?;
	rendered += &renderer.render_metric(
		"XOFFBATT",
		MetricParseConfig {
			parse_type: MetricParseType::Timestamp,
			special_values: [("N/A", None)].into(),
		},
		"apcupsd_last_transfer_off_battery_timestamp_seconds",
		"Date, time of last transfer off battery since apcupsd startup.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"LASTSTEST",
		MetricParseType::Timestamp,
		"apcupsd_last_self_test_timestamp_seconds",
		"Date, time of last self test.",
		MetricType::Gauge,
	)?;
	if let Some(stat_renderer) = renderer.bitfield_renderer::<u32>("STATFLAG")? {
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_calibration",
			"Runtime calibration occurring.",
			apcupsd_bitmasks::status::UPS_CALIBRATION,
		);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_trim", "SmartTrim.", apcupsd_bitmasks::status::UPS_TRIM);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_boost", "SmartBoost.", apcupsd_bitmasks::status::UPS_BOOST);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_on_line", "On line.", apcupsd_bitmasks::status::UPS_ONLINE);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_on_battery", "On battery.", apcupsd_bitmasks::status::UPS_ONBATT);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_overloaded_output",
			"Overloaded output.",
			apcupsd_bitmasks::status::UPS_OVERLOAD,
		);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_battery_low", "Battery low.", apcupsd_bitmasks::status::UPS_BATTLOW);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_replace_battery",
			"Replace battery.",
			apcupsd_bitmasks::status::UPS_REPLACEBATT,
		);

		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_communication_lost",
			"Communications with UPS lost.",
			apcupsd_bitmasks::status::UPS_COMMLOST,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_shutdown_in_progress",
			"Shutdown in progress.",
			apcupsd_bitmasks::status::UPS_SHUTDOWN,
		);
		rendered += &stat_renderer.render_bitfield_metric("apcupsd_status_slave", "Set if this is a slave.", apcupsd_bitmasks::status::UPS_SLAVE);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_slave_down",
			"Slave not responding.",
			apcupsd_bitmasks::status::UPS_SLAVEDOWN,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_on_battery_message_sent",
			"Set when UPS_ONBATT message is sent.",
			apcupsd_bitmasks::status::UPS_ONBATT_MSG,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_fast_poll",
			"Set on power failure to poll faster.",
			apcupsd_bitmasks::status::UPS_FASTPOLL,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_shutdown_load",
			"Set when BatLoad <= percent.",
			apcupsd_bitmasks::status::UPS_SHUT_LOAD,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_shutdown_time",
			"Set when time on batts > maxtime.",
			apcupsd_bitmasks::status::UPS_SHUT_BTIME,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_shutdown_time_left",
			"Set when TimeLeft <= runtime.",
			apcupsd_bitmasks::status::UPS_SHUT_LTIME,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_emergency_shutdown",
			"Set when battery power has failed.",
			apcupsd_bitmasks::status::UPS_SHUT_EMERG,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_remote_shutdown",
			"Set when remote shutdown.",
			apcupsd_bitmasks::status::UPS_SHUT_REMOTE,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_plugged_in",
			"Set if computer is plugged into UPS.",
			apcupsd_bitmasks::status::UPS_PLUGGED,
		);
		rendered += &stat_renderer.render_bitfield_metric(
			"apcupsd_status_battery_present",
			"Indicates if battery is connected.",
			apcupsd_bitmasks::status::UPS_BATTPRESENT,
		);
	}
	if let Some(dip_switch_renderer) = renderer.bitfield_renderer::<u8>("DIPSW")? {
		rendered += &dip_switch_renderer.render_bitfield_metric(
			"apcupsd_status_low_battery_alarm_delayed",
			"Low battery alarm changed from 2 to 5 mins. Autostartup disabled on SU370ci and 400.",
			apcupsd_bitmasks::dip_switch::LOW_BATTERY_5_MIN,
		);
		rendered += &dip_switch_renderer.render_bitfield_metric(
			"apcupsd_status_audible_alarm_delayed",
			"Audible alarm delayed 30 seconds.",
			apcupsd_bitmasks::dip_switch::ALARM_DELAY_30_SEC,
		);
		rendered += &dip_switch_renderer.render_bitfield_metric(
			"apcupsd_status_output_transfer_voltage_changed",
			"Output transfer set to 115 VAC (from 120 VAC) or to 240 VAC (from 230 VAC).",
			apcupsd_bitmasks::dip_switch::OUTPUT_TRANSFER_115_240_VOLTS,
		);
		rendered += &dip_switch_renderer.render_bitfield_metric(
			"apcupsd_status_input_voltage_range_expanded",
			"UPS desensitized - input voltage range expanded.",
			apcupsd_bitmasks::dip_switch::INPUT_VOLTAGE_RANGE_EXPANDED,
		);
	}
	if let Some(register_one_renderer) = renderer.bitfield_renderer::<u8>("REG1")? {
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_wakeup_mode",
			"In wakeup mode (typically lasts < 2s).",
			apcupsd_bitmasks::register_one::WAKEUP_MODE,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_mode_from_internal_fault",
			"In bypass mode due to internal fault.",
			apcupsd_bitmasks::register_one::BYPASS_MODE_INTERNAL_FAULT,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_entering_bypass_mode_from_command",
			"Going to bypass mode due to command.",
			apcupsd_bitmasks::register_one::ENTERING_BYPASS_MODE_COMMAND,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_in_bypass_mode_from_command",
			"In bypass mode due to command.",
			apcupsd_bitmasks::register_one::IN_BYPASS_MODE_COMMAND,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_leaving_bypass_mode",
			"Returning from bypass mode.",
			apcupsd_bitmasks::register_one::LEAVING_BYPASS_MODE,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_in_bypass_mode_from_manual_control",
			"In bypass mode due to manual bypass control.",
			apcupsd_bitmasks::register_one::IN_BYPASS_MODE_MANUAL,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_ready_power_load_on_command",
			"Ready to power load on user command.",
			apcupsd_bitmasks::register_one::READY_POWER_LOAD_COMMAND,
		);
		rendered += &register_one_renderer.render_bitfield_metric(
			"apcupsd_status_ready_power_load_on_command_or_line",
			"Ready to power load on user command or return of line power.",
			apcupsd_bitmasks::register_one::READY_POWER_LOAD_COMMAND_OR_LINE,
		);
	}
	if let Some(register_two_renderer) = renderer.bitfield_renderer::<u8>("REG2")? {
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_mode_from_electronics_fan_failure",
			"Fan failure in electronics, UPS in bypass.",
			apcupsd_bitmasks::register_two::BYPASS_MODE_FAN_FAILURE,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_isolation_unit_fan_failure",
			"Fan failure in isolation unit.",
			apcupsd_bitmasks::register_two::FAN_FAILURE_ISOLATION_UNIT,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_supply_failure",
			"Bypass supply failure.",
			apcupsd_bitmasks::register_two::BYPASS_SUPPLY_FAILURE,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_mode_from_output_voltage_select_failure",
			"Output voltage select failure, UPS in bypass.",
			apcupsd_bitmasks::register_two::BYPASS_MODE_OUTPUT_VOLTAGE_SELECT_FAILURE,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_mode_from_dc_imbalance",
			"DC imbalance, UPS in bypass.",
			apcupsd_bitmasks::register_two::BYPASS_MODE_DC_IMBALANCE,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_battery_disconnected",
			"Battery is disconnected.",
			apcupsd_bitmasks::register_two::BATTERY_DISCONNECTED,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_relay_fault_smarttrim_or_smartboost",
			"Relay fault in SmartTrim or SmartBoost.",
			apcupsd_bitmasks::register_two::RELAY_FAULT_SMARTTRIM_SMARTBOOST,
		);
		rendered += &register_two_renderer.render_bitfield_metric(
			"apcupsd_status_bad_output_voltage",
			"Bad output voltage.",
			apcupsd_bitmasks::register_two::BAD_OUTPUT_VOLTAGE,
		);
	}
	if let Some(register_three_renderer) = renderer.bitfield_renderer::<u8>("REG3")? {
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_output_unpowered_from_low_battery_shutdown",
			"Output unpowered due to shutdown by low battery.",
			apcupsd_bitmasks::register_three::OUTPUT_UNPOWERED_LOW_BATTERY,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_cannot_transfer_to_battery_due_to_overload",
			"Unable to transfer to battery due to overload.",
			apcupsd_bitmasks::register_three::NO_TRANSFER_OVERLOAD,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_ups_off_from_main_relay_failure",
			"Main relay malfunction - UPS turned off.",
			apcupsd_bitmasks::register_three::RELAY_MALFUNCTION_POWER_OFF,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_sleep_mode_from_command",
			"In sleep mode from @ command (maybe others).",
			apcupsd_bitmasks::register_three::SLEEP_MODE_COMMAND,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_shutdown_mode_from_command",
			"In shutdown mode from S command.",
			apcupsd_bitmasks::register_three::SHUTDOWN_MODE_COMMAND,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_battery_charger_failure",
			"Battery charger failure.",
			apcupsd_bitmasks::register_three::BATTERY_CHARGER_FAILURE,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_bypass_relay_failure",
			"Bypass relay malfunction.",
			apcupsd_bitmasks::register_three::BYPASS_RELAY_FAILURE,
		);
		rendered += &register_three_renderer.render_bitfield_metric(
			"apcupsd_status_operating_temperature_exceeded",
			"Normal operating temperature exceeded.",
			apcupsd_bitmasks::register_three::OPERATING_TEMPERATURE_EXCEEDED,
		);
	}
	rendered += &renderer.render_metric(
		"BATTDATE",
		MetricParseType::Date,
		"apcupsd_battery_last_replacement_timestamp_seconds",
		"Date battery last replaced.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NOMOUTV",
		MetricParseType::Voltage,
		"apcupsd_battery_nominal_output_volts",
		"Nominal output voltage to supply when on battery power.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NOMINV",
		MetricParseType::Voltage,
		"apcupsd_line_nominal_volts",
		"Nominal AC input line voltage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NOMBATTV",
		MetricParseType::Voltage,
		"apcupsd_battery_nominal_volts",
		"Nominal battery voltage.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NOMPOWER",
		MetricParseType::Power,
		"apcupsd_nominal_power_watts",
		"Nominal power output in watts.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"NOMAPNT",
		MetricParseType::ApparentPower,
		"apcupsd_apparent_power_volt_amps",
		"Apparent power output in volt-amperes.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"HUMIDITY",
		MetricParseType::Percentage,
		"apcupsd_humidity_percent",
		"Ambient humidity.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"AMBTEMP",
		MetricParseType::Temperature,
		"apcupsd_ambient_temperature_celsius",
		"Ambient temperature.",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"EXTBATTS",
		MetricParseType::Count,
		"apcupsd_external_battery_count",
		"Number of external batteries (for XL models).",
		MetricType::Gauge,
	)?;
	rendered += &renderer.render_metric(
		"BADBATTS",
		MetricParseType::Count,
		"apcupsd_external_battery_bad_count",
		"Number of bad external battery packs (for XL models).",
		MetricType::Gauge,
	)?;

	let mut apcupsd_data = renderer.into_remaining_data();
	for ignored in ["APC", "STATUS", "END APC"] {
		apcupsd_data.remove(ignored);
	}

	if !apcupsd_data.is_empty() {
		eprintln!("Unknown keys: {:?}", apcupsd_data.keys());
	}

	Ok(rendered)
}

struct MetricRenderer {
	labels: Vec<(String, String)>,
	apcupsd_data: HashMap<String, String>,
}

impl MetricRenderer {
	pub fn new(labels: Vec<(String, String)>, apcupsd_data: HashMap<String, String>) -> Self {
		Self { labels, apcupsd_data }
	}

	pub fn render_metric(
		&mut self,
		key: &str,
		parse_config: impl Into<MetricParseConfig>,
		name: &str,
		help: &str,
		metric_type: MetricType,
	) -> Result<String, RenderMetricsError> {
		if let Some(parse_result) = self.apcupsd_data.remove(key).and_then(|v| parse_metric(v, parse_config.into()).transpose()) {
			Ok(PrometheusMetric::build()
				.with_name(name)
				.with_help(help)
				.with_metric_type(metric_type)
				.build()
				.render_and_append_instance(&prometheus_instance_with_labels(&self.labels).with_value(parse_result.map_err(|e| {
					RenderMetricsError::ParseMetricError {
						key: key.to_string(),
						error: e,
					}
				})?))
				.render())
		} else {
			Ok(String::new())
		}
	}

	pub fn bitfield_renderer<T: BitfieldType>(&mut self, key: &str) -> Result<Option<BitfieldMetricRenderer<T>>, RenderMetricsError> {
		if let Some(hex) = self.apcupsd_data.remove(key) {
			let bitfield =
				hex.get(2..).map(|h| T::from_str_radix(h, 16)).transpose().ok().flatten().ok_or_else(|| RenderMetricsError::ParseMetricError {
					key: key.to_string(),
					error: ParseMetricError::InvalidHex(hex),
				})?;
			Ok(Some(BitfieldMetricRenderer {
				labels: self.labels.clone(),
				bitfield,
			}))
		} else {
			Ok(None)
		}
	}

	pub fn into_remaining_data(self) -> HashMap<String, String> {
		self.apcupsd_data
	}
}

trait BitfieldType: Unsigned + BitAnd<Self, Output = Self> + PartialEq + Copy {}
impl<T: Unsigned + BitAnd<Self, Output = Self> + PartialEq + Copy> BitfieldType for T {}

struct BitfieldMetricRenderer<T: BitfieldType> {
	labels: Vec<(String, String)>,
	bitfield: T,
}

impl<T: BitfieldType> BitfieldMetricRenderer<T> {
	pub fn render_bitfield_metric(&self, name: &str, help: &str, mask: T) -> String {
		PrometheusMetric::build()
			.with_name(name)
			.with_help(help)
			.with_metric_type(MetricType::Gauge)
			.build()
			.render_and_append_instance(&prometheus_instance_with_labels(&self.labels).with_value(f64::from(self.bitfield & mask != T::zero())))
			.render()
	}
}

#[derive(Error, Debug)]
enum RenderMetricsError {
	#[error("{key}: {error}")]
	ParseMetricError { key: String, error: ParseMetricError },
}

struct MetricParseConfig {
	parse_type: MetricParseType,
	special_values: HashMap<&'static str, Option<f64>>,
}

enum MetricParseType {
	Timestamp,
	Date,
	Duration,
	Percentage,
	Voltage,
	Temperature,
	Frequency,
	Current,
	Count,
	Power,
	ApparentPower,
}

impl From<MetricParseType> for MetricParseConfig {
	fn from(value: MetricParseType) -> Self {
		Self {
			parse_type: value,
			special_values: HashMap::new(),
		}
	}
}

fn parse_metric(value: String, parse_config: MetricParseConfig) -> Result<Option<f64>, ParseMetricError> {
	if let Some(special_value) = parse_config.special_values.get(value.as_str()) {
		return Ok(*special_value);
	}
	match parse_config.parse_type {
		MetricParseType::Timestamp => {
			DateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S %z")
				.or_else(|_| DateTime::parse_from_str(&value, "%a %b %d %X %z %Y")) // Historic apcupsd date format
				.map(|t| Some(t.timestamp() as f64))
				.map_err(|e| ParseMetricError::InvalidTimestamp(value, e.to_string()))
		},
		MetricParseType::Date => NaiveDate::parse_from_str(&value, "%Y-%m-%d")
			.or_else(|_| NaiveDate::parse_from_str(&value, "%m/%d/%y"))
			.map(|t| Some(t.and_time(NaiveTime::MIN).and_utc().timestamp() as f64))
			.map_err(|e| ParseMetricError::InvalidDate(value, e.to_string())),
		MetricParseType::Duration => match value.split_once(" ") {
			Some((s, "Seconds")) => s.parse::<f64>().map(Some).map_err(|_| ()),
			Some((s, "Minutes")) => s.parse::<f64>().map(|m| Some(m * 60.)).map_err(|_| ()),
			Some((_, _)) => Err(()),
			None => Err(()),
		}
		.map_err(|_| ParseMetricError::InvalidDuration(value)),
		MetricParseType::Percentage => match value.strip_suffix(" Percent") {
			Some(v) => v.parse::<f64>().map(|v| Some(v / 100.)).map_err(|_| ParseMetricError::InvalidPercentage(value)),
			None => Err(ParseMetricError::InvalidPercentage(value)),
		},
		MetricParseType::Voltage => match value.strip_suffix(" Volts") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidVoltage(value)),
			None => Err(ParseMetricError::InvalidVoltage(value)),
		},
		MetricParseType::Temperature => match value.strip_suffix(" C") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidTemperature(value)),
			None => Err(ParseMetricError::InvalidTemperature(value)),
		},
		MetricParseType::Frequency => match value.strip_suffix(" Hz") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidFrequency(value)),
			None => Err(ParseMetricError::InvalidFrequency(value)),
		},
		MetricParseType::Current => match value.strip_suffix(" Amps") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidCurrent(value)),
			None => Err(ParseMetricError::InvalidCurrent(value)),
		},
		MetricParseType::Count => value.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidCount(value)),
		MetricParseType::Power => match value.strip_suffix(" Watts") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidPower(value)),
			None => Err(ParseMetricError::InvalidPower(value)),
		},
		MetricParseType::ApparentPower => match value.strip_suffix(" VA") {
			Some(v) => v.parse::<f64>().map(Some).map_err(|_| ParseMetricError::InvalidApparentPower(value)),
			None => Err(ParseMetricError::InvalidApparentPower(value)),
		},
	}
}

#[allow(clippy::enum_variant_names)]
#[derive(Error, Debug)]
enum ParseMetricError {
	#[error("invalid timestamp \"{0}\" {1}")]
	InvalidTimestamp(String, String),
	#[error("invalid date \"{0}\" {1}")]
	InvalidDate(String, String),
	#[error("invalid duration \"{0}\"")]
	InvalidDuration(String),
	#[error("invalid percentage \"{0}\"")]
	InvalidPercentage(String),
	#[error("invalid voltage \"{0}\"")]
	InvalidVoltage(String),
	#[error("invalid temperature \"{0}\"")]
	InvalidTemperature(String),
	#[error("invalid frequency \"{0}\"")]
	InvalidFrequency(String),
	#[error("invalid current \"{0}\"")]
	InvalidCurrent(String),
	#[error("invalid count \"{0}\"")]
	InvalidCount(String),
	#[error("invalid power \"{0}\"")]
	InvalidPower(String),
	#[error("invalid apparent power \"{0}\"")]
	InvalidApparentPower(String),
	#[error("invalid hex value \"{0}\"")]
	InvalidHex(String),
}

/// Throttle the number of times data is fetched from apcupsd, returning previous data instead if the wait time hasn't been reached.
#[derive(Clone)]
struct APCThrottledAccess {
	inner: Arc<Mutex<APCThrottledAccessInner>>,
}

struct APCThrottledAccessInner {
	apc_access: APCAccess,
	wait_time: Duration,
	last_call: Instant,
	data: Result<HashMap<String, String>, std::io::ErrorKind>,
}

impl APCThrottledAccess {
	pub fn new(config: APCAccessConfig, wait_time: Duration) -> Self {
		Self {
			inner: Arc::new(Mutex::new(APCThrottledAccessInner {
				apc_access: APCAccess::new(Some(config)),
				wait_time,
				last_call: Instant::now() - wait_time,
				data: Ok(HashMap::new()),
			})),
		}
	}

	pub async fn fetch(&mut self) -> Result<HashMap<String, String>, std::io::ErrorKind> {
		let mut inner = self.inner.lock().await;
		if inner.last_call.elapsed() >= inner.wait_time {
			let apc_access = inner.apc_access.clone();
			inner.data = spawn_blocking(move || apc_access.fetch().map_err(|e| e.kind())).await.unwrap_or_else(|_| Ok(HashMap::new()));
			inner.last_call = Instant::now();
		}
		inner.data.clone()
	}
}

#[cfg(test)]
mod tests {
	use std::{
		collections::HashMap,
		fs::File,
		io::{BufRead, BufReader},
		path::PathBuf,
	};

	use insta::with_settings;
	use rstest::rstest;

	use crate::{render_metrics, RenderMetricsError};

	#[rstest]
	fn test_examples(#[files("tests/*_examples/*.status")] path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
		let test_data = BufReader::new(File::open(path.clone())?)
			.lines()
			.map(|lr| lr.map(|l| l.split_once(":").ok_or("invalid test file").map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))))
			.collect::<Result<Result<HashMap<_, _>, _>, _>>()??;
		with_settings!(
			{
				prepend_module_to_snapshot => false,
				snapshot_path => "../tests/snapshots",
				snapshot_suffix => (|| Some([path.parent()?.file_name()?.to_str()?, path.file_name()?.to_str()?].join("/")))().ok_or("bad filename")?
			},
			{ Ok::<_, RenderMetricsError>(insta::assert_snapshot!(render_metrics(test_data)?)) }
		)?;
		Ok(())
	}
}
