use tracing_subscriber::filter::LevelFilter;
pub use tracing::{self, instrument, span, Level};
use tracing_subscriber::{
	fmt::{format::FmtSpan, SubscriberBuilder},
	layer::{Layer, SubscriberExt},
    filter::FilterFn,
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

pub fn setup_json_log(out: std::fs::File) {
	let _ = SubscriberBuilder::default()
		.json()
		.with_line_number(true)
		.with_file(true)
		.with_span_events(FmtSpan::FULL)
		.with_env_filter(EnvFilter::from_default_env())
		.with_writer(out)
		.finish()
		.try_init();
}


#[macro_export]
macro_rules! define_span {
	($tag:expr, $id:tt, $level:expr) => {
		#[cfg(feature = "debug-tracing")]
		let span = dkg_logging::span!($level, $tag, $id);
		#[cfg(feature = "debug-tracing")]
		let _enter = span.enter();
	};
	($tag:expr, $id:tt) => {
		$crate::define_span!($tag, $id, dkg_logging::Level::TRACE);
	};
}
