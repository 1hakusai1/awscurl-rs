use std::{
    process::{self},
    time::SystemTime,
};

use anyhow::Context;
use aws_credential_types::provider::ProvideCredentials;
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
    data: Option<String>,

    #[arg(short = 'X', long = "request")]
    method: Option<String>,

    #[arg(short = 'H', long)]
    header: Vec<String>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    service: Option<String>,
}

impl Args {
    fn build_unsigned_request(&self) -> anyhow::Result<http::Request<String>> {
        let mut builder = http::Request::builder();
        for raw_string in self.header.iter() {
            let (key, value) = match raw_string.split_once(":") {
                Some(pair) => pair,
                None => return Err(anyhow::anyhow!(format!("Invalid header: {}", raw_string))),
            };
            builder = builder.header(key, value);
        }
        let method = match (self.method.clone(), self.data.clone()) {
            (Some(method), _) => method,
            (None, Some(_)) => "POST".to_string(),
            (None, None) => "GET".to_string(),
        };
        let req = builder
            .uri(self.url.clone())
            .method(method.as_bytes())
            .body(self.data.clone().unwrap_or("".to_string()))?;
        Ok(req)
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
    let confg = aws_config::from_env().load().await;
    let identity = confg
        .credentials_provider()
        .context("Unable to find credentials")?
        .provide_credentials()
        .await?
        .into();
    let singing_settings = SigningSettings::default();
    let service = args.service.clone().unwrap_or("execute-api".to_string());
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .time(SystemTime::now())
        .settings(singing_settings)
        .region(confg.region().context("Unable to decide region")?.as_ref())
        .name(&service)
        .build()?
        .into();
    let mut unsigned_request = args.build_unsigned_request()?;
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
    if args.verbose {
        print_request_verbose(&reqwest_req);
    }

    let res = reqwest::Client::new().execute(reqwest_req).await?;
    if args.verbose {
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
