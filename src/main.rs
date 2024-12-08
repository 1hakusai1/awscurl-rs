use std::{
    process::{self},
    time::SystemTime,
};

use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use clap::Parser;

#[derive(Debug)]
struct Error(String);

type Result<T> = std::result::Result<T, Error>;

#[derive(Parser, Debug)]
struct Args {
    url: String,

    #[arg(short, long)]
    data: Option<String>,

    #[arg(short = 'X', long = "request")]
    method: Option<String>,

    #[arg(short = 'H', long)]
    header: Vec<String>,
}

impl Args {
    fn build_unsigned_request(&self) -> Result<http::Request<String>> {
        let mut builder = http::Request::builder();
        for raw_string in self.header.iter() {
            let (key, value) = match raw_string.split_once(":") {
                Some(pair) => pair,
                None => return Err(Error(format!("Invalid header: {}", raw_string))),
            };
            builder = builder.header(key, value);
        }
        builder
            .uri(self.url.clone())
            .method(self.method.clone().unwrap_or("GET".to_string()).as_bytes())
            .body(self.data.clone().unwrap_or("".to_string()))
            .map_err(|_| Error("Failed to build request".to_string()))
    }
}

#[tokio::main]
async fn main() {
    match inner().await {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e.0);
            process::exit(1)
        }
    }
}

async fn inner() -> Result<()> {
    let args = Args::parse();
    let confg = aws_config::from_env().load().await;
    let identity = confg
        .credentials_provider()
        .ok_or(Error("Unable to find credentials".to_string()))?
        .provide_credentials()
        .await
        .map_err(|_| Error("Unable to retrieve credentials".to_string()))?
        .into();
    let singing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .time(SystemTime::now())
        .settings(singing_settings)
        .region(
            confg
                .region()
                .ok_or(Error("Unable to decide region".to_string()))?
                .as_ref(),
        )
        .name("execute-api")
        .build()
        .map_err(|_| Error("Unable to build signing params".to_string()))?
        .into();
    let mut unsigned_request = args
        .build_unsigned_request()
        .map_err(|_| Error("Failed to build request".to_string()))?;
    let signable_request = SignableRequest::new(
        unsigned_request.method().as_str(),
        unsigned_request.uri().to_string(),
        unsigned_request
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
        SignableBody::Bytes(unsigned_request.body().as_bytes()),
    )
    .map_err(|_| Error("Unable to build singing a request".to_string()))?;
    let (instruction, _signature) = sign(signable_request, &signing_params)
        .map_err(|_| Error("Unable to sign the request".to_string()))?
        .into_parts();

    instruction.apply_to_request_http1x(&mut unsigned_request);
    let reqwest_req: reqwest::Request = unsigned_request
        .try_into()
        .map_err(|_| Error("Unable to build a request".to_string()))?;
    let res = reqwest::Client::new()
        .execute(reqwest_req)
        .await
        .map_err(|_| Error("Request failed".to_string()))?;
    println!(
        "{}",
        res.text()
            .await
            .map_err(|_| Error("Failed to parse the response".to_string()))?
    );
    Ok(())
}
