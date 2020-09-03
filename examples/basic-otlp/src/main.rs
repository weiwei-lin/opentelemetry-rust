use futures::stream::{Stream, StreamExt};
use opentelemetry::api::metrics::{self, MetricsError, ObserverResult};
use opentelemetry::api::{BaggageExt, Context, Key, KeyValue, TraceContextExt, Tracer};
use opentelemetry::exporter;
use opentelemetry::sdk::metrics::PushController;
use opentelemetry::{global, sdk};
use std::time::Duration;

fn init_tracer() -> Result<sdk::Tracer, Box<dyn std::error::Error>> {
    opentelemetry_otlp::new_pipeline().install()
}

// Skip first immediate tick from tokio, not needed for async_std.
fn delayed_interval(duration: Duration) -> impl Stream<Item = tokio::time::Instant> {
    tokio::time::interval(duration).skip(1)
}

fn init_meter() -> metrics::Result<PushController> {
    exporter::metrics::stdout(tokio::spawn, delayed_interval)
        .with_quantiles(vec![0.5, 0.9, 0.99])
        .with_formatter(|batch| {
            serde_json::to_value(batch)
                .map(|value| value.to_string())
                .map_err(|err| MetricsError::Other(err.to_string()))
        })
        .try_init()
}

const FOO_KEY: Key = Key::from_static_str("ex.com/foo");
const BAR_KEY: Key = Key::from_static_str("ex.com/bar");
const LEMONS_KEY: Key = Key::from_static_str("ex.com/lemons");
const ANOTHER_KEY: Key = Key::from_static_str("ex.com/another");

lazy_static::lazy_static! {
    static ref COMMON_LABELS: [KeyValue; 4] = [
        LEMONS_KEY.i64(10),
        KeyValue::new("A", "1"),
        KeyValue::new("B", "2"),
        KeyValue::new("C", "3"),
    ];
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = init_tracer()?;
    let _started = init_meter()?;

    let tracer = global::tracer("ex.com/basic");
    let meter = global::meter("ex.com/basic");

    let one_metric_callback = |res: ObserverResult<f64>| res.observe(1.0, COMMON_LABELS.as_ref());
    let _ = meter
        .f64_value_observer("ex.com.one", one_metric_callback)
        .with_description("A ValueObserver set to 1.0")
        .init();

    let value_recorder_two = meter.f64_value_recorder("ex.com.two").init();

    let _correlations =
        Context::current_with_baggage(vec![FOO_KEY.string("foo1"), BAR_KEY.string("bar1")])
            .attach();

    let value_recorder = value_recorder_two.bind(COMMON_LABELS.as_ref());

    tracer.in_span("operation", |cx| {
        let span = cx.span();
        span.add_event(
            "Nice operation!".to_string(),
            vec![Key::new("bogons").i64(100)],
        );
        span.set_attribute(ANOTHER_KEY.string("yes"));

        meter.record_batch_with_context(
            // Note: call-site variables added as context Entries:
            &Context::current_with_baggage(vec![ANOTHER_KEY.string("xyz")]),
            COMMON_LABELS.as_ref(),
            vec![value_recorder_two.measurement(2.0)],
        );

        tracer.in_span("Sub operation...", |cx| {
            let span = cx.span();
            span.set_attribute(LEMONS_KEY.string("five"));

            span.add_event("Sub span event".to_string(), vec![]);

            value_recorder.record(1.3);
        });
    });

    Ok(())
}