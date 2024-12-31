use std::{
    process::{self},
    time::SystemTime,
};

use anyhow::Context;
use aws_config::SdkConfig;
use aws_credential_types::{provider::ProvideCredentials, Credentials};
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    url: String,

    #[arg(short, long)]
    /// Request body
    data: Option<String>,

    #[arg(short = 'X', long = "request")]
    /// HTTP method (Ex. GET, POST, PUT ...)
    method: Option<String>,

    #[arg(short = 'H', long)]
    /// HTTP headers (Ex. content-type: application/json)
    header: Vec<String>,

    #[arg(long)]
    /// AWS service name (Default: execute-api)
    service: Option<String>,

    #[arg(long)]
    /// AWS region
    region: Option<String>,

    #[arg(long)]
    /// AWS profile
    profile: Option<String>,

    #[arg(short, long)]
    verbose: bool,
}

struct AwsCurlParam {
    args: Args,
    config: SdkConfig,
    time: SystemTime,
}
const DEFAULT_SERVICE: &str = "execute-api";

impl AwsCurlParam {
    fn new(args: Args, config: SdkConfig, time: SystemTime) -> Self {
        Self { args, config, time }
    }

    fn service(&self) -> &str {
        self.args.service.as_deref().unwrap_or(DEFAULT_SERVICE)
    }

    fn region(&self) -> Option<&str> {
        let config_region = self.config.region().map(|r| r.as_ref());
        self.args.region.as_deref().or(config_region)
    }

    async fn credentials(&self) -> anyhow::Result<Credentials> {
        let config = self
            .config
            .credentials_provider()
            .context("Unable to find credentials")?
            .provide_credentials()
            .await?;
        Ok(config)
    }

    async fn build_request(&self) -> anyhow::Result<reqwest::Request> {
        let identity = self.credentials().await?.into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .time(self.time)
            .settings(SigningSettings::default())
            .region(self.region().context("Unable to decide region")?)
            .name(self.service())
            .build()?
            .into();

        let mut unsigned_request = {
            let args: &Args = &self.args;
            let mut builder = http::Request::builder();
            for raw_string in args.header.iter() {
                let (key, value) = match raw_string.split_once(":") {
                    Some(pair) => pair,
                    None => return Err(anyhow::anyhow!(format!("Invalid header: {}", raw_string))),
                };
                builder = builder.header(key, value);
            }
            let method = match (args.method.clone(), args.data.clone()) {
                (Some(method), _) => method,
                (None, Some(_)) => "POST".to_string(),
                (None, None) => "GET".to_string(),
            };
            let req = builder
                .uri(args.url.clone())
                .method(method.as_bytes())
                .body(args.data.clone().unwrap_or("".to_string()))?;
            req
        };
        let signable_request = SignableRequest::new(
            unsigned_request.method().as_str(),
            unsigned_request.uri().to_string(),
            unsigned_request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
            SignableBody::Bytes(unsigned_request.body().as_bytes()),
        )?;
        let (instruction, _signature) = sign(signable_request, &signing_params)?.into_parts();

        instruction.apply_to_request_http1x(&mut unsigned_request);
        let reqwest_req: reqwest::Request = unsigned_request.try_into()?;
        Ok(reqwest_req)
    }
}

#[tokio::main]
async fn main() {
    let status = inner().await.unwrap_or_else(|e| {
        eprintln!("{:?}", e);
        process::exit(1)
    });
    if !status.is_success() {
        process::exit(1)
    }
}

async fn inner() -> anyhow::Result<http::StatusCode> {
    let args = Args::parse();
    let mut config_loader = aws_config::from_env();
    if let Some(profile) = &args.profile {
        config_loader = config_loader.profile_name(profile);
    }
    let config = config_loader.load().await;
    let param = AwsCurlParam::new(args, config, SystemTime::now());

    let reqwest_req = param.build_request().await?;
    if param.args.verbose {
        print_request_verbose(&reqwest_req);
    }

    let res = reqwest::Client::new().execute(reqwest_req).await?;
    if param.args.verbose {
        print_response_verbose(&res);
    }

    let status = res.status();
    let body = res.text().await?;
    println!("{}", body);
    Ok(status)
}

fn print_request_verbose(req: &reqwest::Request) {
    eprintln!(
        "> {} {} {:?}",
        req.method().as_str(),
        req.url().path(),
        req.version()
    );
    for (key, value) in req.headers() {
        eprintln!("> {} {}", key.as_str(), value.to_str().unwrap())
    }
    eprintln!(">");
}

fn print_response_verbose(res: &reqwest::Response) {
    eprintln!("< {:?} {}", res.version(), res.status().as_str());
    for (key, value) in res.headers() {
        eprintln!("< {} {}", key.as_str(), value.to_str().unwrap())
    }
    eprintln!("<");
}
