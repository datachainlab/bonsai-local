use anyhow::Result;
use clap::Parser;
use opentelemetry::{trace::TracerProvider, KeyValue};
use opentelemetry_sdk::{
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    Resource,
};
use opentelemetry_semantic_conventions::{
    attribute::{SERVICE_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use std::{env, time::Duration};
use tokio::net::TcpListener;
use tracing::debug;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use url::Url;

#[derive(Parser, Debug)]
#[command(name = "bonsai-local")]
#[command(about = "Local Bonsai REST API Server", long_about = None)]
struct Args {
    /// Server URL (must be http:// or https://)
    #[arg(long, value_parser = validate_url)]
    server_url: Option<Url>,

    /// Address to listen on (e.g., "127.0.0.1:8080", "0.0.0.0:8080")
    #[arg(long, default_value = "127.0.0.1:8080", value_name = "ADDRESS")]
    listen_address: String,

    /// Time-to-live for cached entries in seconds (default: 14400 = 4 hours)
    #[arg(long, default_value = "14400", value_name = "SECONDS")]
    ttl: u64,

    /// Channel buffer size for prover queue
    #[arg(long, default_value = "8", value_name = "SIZE")]
    channel_buffer_size: usize,

    /// Required r0vm version (format: <major>.<minor>, e.g., "1.0", "1.2")
    #[arg(long, value_name = "VERSION")]
    r0vm_version: Option<String>,
}

fn validate_url(s: &str) -> Result<Url, String> {
    let url = Url::parse(s).map_err(|e| format!("Invalid URL: {e}"))?;

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err("URL must use http:// or https:// scheme".to_string()),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let otel_enabled = env::var("BONSAI_OTEL_ENABLE")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let builder = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing::Level::INFO.as_str().into()),
        )
        .with(tracing_subscriber::fmt::layer());
    let shutdown_fn = if otel_enabled {
        let tracer_provider = init_tracer_provider();
        builder
            .with(OpenTelemetryLayer::new(tracer_provider.tracer("bonsai")))
            .init();
        Some(move || {
            let _ = tracer_provider.shutdown();
        })
    } else {
        builder.init();
        None
    };

    // Check Docker availability
    bonsai_local::version::check_docker()?;
    debug!("Docker check passed");

    // Check r0vm version if specified
    if let Some(ref required_version) = args.r0vm_version {
        bonsai_local::version::check_r0vm_version(required_version)?;
        debug!("r0vm version check passed: {}", required_version);
    }

    let listener = TcpListener::bind(&args.listen_address).await?;
    let options = bonsai_local::ServerOptions {
        server_url: args.server_url,
        ttl: Duration::from_secs(args.ttl),
        channel_buffer_size: args.channel_buffer_size,
    };
    bonsai_local::serve(listener, options).await?;
    if let Some(f) = shutdown_fn {
        f()
    }
    Ok(())
}

fn init_tracer_provider() -> SdkTracerProvider {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .unwrap();

    SdkTracerProvider::builder()
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            1.0,
        ))))
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource())
        .with_batch_exporter(exporter)
        .build()
}

fn resource() -> Resource {
    Resource::builder()
        .with_service_name(env!("CARGO_PKG_NAME"))
        .with_schema_url(
            [
                KeyValue::new(SERVICE_NAME, env!("CARGO_PKG_NAME")),
                KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
            ],
            SCHEMA_URL,
        )
        .build()
}
