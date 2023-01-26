pub use tracing::{self, debug, error, info, instrument, trace, warn};
use tracing_subscriber::{
	fmt::{format::FmtSpan, SubscriberBuilder},
	util::SubscriberInitExt,
	EnvFilter,
};

pub fn setup_log() {
	let _ = SubscriberBuilder::default()
		.with_line_number(true)
		.with_file(true)
		.with_span_events(FmtSpan::FULL)
		.with_env_filter(EnvFilter::from_default_env())
		.finish()
		.try_init();
}
