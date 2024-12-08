use std::time::SystemTime;

use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::{
    http_request::{sign, SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    url: String,

    #[arg(short, long)]
    data: Option<String>,

    #[arg(short = 'X', long = "request")]
    method: Option<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let url = &args.url;
    let method = args.method.unwrap_or("GET".to_string());
    let body = args.data.unwrap_or("".to_string());
    let confg = aws_config::from_env().load().await;
    let identity = confg
        .credentials_provider()
        .unwrap()
        .provide_credentials()
        .await
        .unwrap()
        .into();
    let singing_settings = SigningSettings::default();
    let signing_params = v4::SigningParams::builder()
        .identity(&identity)
        .time(SystemTime::now())
        .settings(singing_settings)
        .region(confg.region().unwrap().as_ref())
        .name("execute-api")
        .build()
        .unwrap()
        .into();
    let signable_request = SignableRequest::new(
        &method,
        url,
        std::iter::empty(),
        SignableBody::Bytes(body.as_bytes()),
    )
    .unwrap();
    let (instruction, _signature) = sign(signable_request, &signing_params)
        .unwrap()
        .into_parts();
    let mut http_req = http::Request::builder()
        .method(method.as_bytes())
        .uri(url)
        .body(body)
        .unwrap();
    instruction.apply_to_request_http1x(&mut http_req);
    let reqwest_req: reqwest::Request = http_req.try_into().unwrap();
    let res = reqwest::Client::new().execute(reqwest_req).await.unwrap();
    println!("{}", res.text().await.unwrap());
}
